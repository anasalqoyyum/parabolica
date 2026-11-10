//! A per-user named pipe that makes the first process the primary instance. Later launches send
//! their request bytes to it and exit.
//!
//! The pipe's DACL admits only the current user, remote clients are rejected, and a client checks
//! that the server process runs as the same user before it writes anything.

#![allow(unsafe_code)]

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetLengthSid, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE,
    FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::core::{HSTRING, PWSTR};

/// The largest request a running instance accepts.
pub const MAX_REQUEST_BYTES: usize = 1 << 20;
const ACCEPTED: u8 = 1;
const REJECTED: u8 = 0;
const CONNECT_WAIT_MS: u32 = 2_000;

#[derive(Debug)]
pub enum Claim {
    /// This process owns the name. Start the server to receive later launches' requests.
    Primary(InstanceServer),
    /// Another process of the same user owns the name.
    Running(RunningInstance),
}

/// Claims `name` (a pipe name without the `\\.\pipe\` prefix) for this process.
pub fn claim(name: &str) -> io::Result<Claim> {
    let path = pipe_path(name);
    match create_instance(&path) {
        Ok(pipe) => Ok(Claim::Primary(InstanceServer { path, pipe })),
        // FILE_FLAG_FIRST_PIPE_INSTANCE fails with access denied when the pipe already exists.
        Err(error)
            if [ERROR_ACCESS_DENIED.0, ERROR_PIPE_BUSY.0]
                .map(|code| Some(code as i32))
                .contains(&error.raw_os_error()) =>
        {
            Ok(Claim::Running(RunningInstance { path }))
        }
        Err(error) => Err(error),
    }
}

/// The primary instance's pipe, not yet serving.
pub struct InstanceServer {
    path: HSTRING,
    pipe: Handle,
}

impl fmt::Debug for InstanceServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstanceServer")
            .field("path", &self.path.to_string())
            .finish_non_exhaustive()
    }
}

impl InstanceServer {
    /// Receives requests on a background thread. `on_request` gets each complete request of at
    /// most [`MAX_REQUEST_BYTES`] and returns whether it was accepted; larger ones are rejected
    /// unread. The pipe keeps one instance for the server's life, so no other process can take the
    /// name while it runs.
    pub fn serve(self, on_request: impl FnMut(Vec<u8>) -> bool + Send + 'static) -> ServerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let Self { path, pipe } = self;
        let thread = std::thread::Builder::new()
            .name("parabolica-instance".into())
            .spawn(move || serve(&pipe, &thread_stop, on_request))
            .expect("the instance server thread starts");
        ServerHandle {
            path,
            stop,
            thread: Some(thread),
        }
    }
}

/// Stops the server and releases the name when dropped.
pub struct ServerHandle {
    path: HSTRING,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerHandle")
            .field("path", &self.path.to_string())
            .finish_non_exhaustive()
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the blocking ConnectNamedPipe; the server sees the flag and exits.
        let _ = open_client(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug)]
pub struct RunningInstance {
    path: HSTRING,
}

impl RunningInstance {
    /// Sends `request` and waits for the running instance to accept it. Lets that instance bring
    /// its window to the foreground.
    pub fn send(&self, request: &[u8]) -> io::Result<()> {
        if request.len() > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the request is too large",
            ));
        }
        let pipe = open_client(&self.path)?;
        let mut server_process = 0;
        // SAFETY: `pipe` is an open client handle; the call writes one u32.
        unsafe { GetNamedPipeServerProcessId(pipe.0, &mut server_process) }.map_err(os_error)?;
        if process_user_sid(server_process)? != current_user_sid_bytes()? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the running instance belongs to another user",
            ));
        }
        // The launching process is in the foreground and may pass that right on. Failure only
        // means the running window may flash instead of coming to the front.
        // SAFETY: plain Win32 call with a process ID.
        let _ = unsafe { AllowSetForegroundWindow(server_process) };
        let length = u32::try_from(request.len()).expect("bounded by MAX_REQUEST_BYTES");
        write_all(&pipe, &length.to_le_bytes())?;
        write_all(&pipe, request)?;
        let mut reply = [REJECTED];
        read_exact(&pipe, &mut reply)?;
        if reply[0] == ACCEPTED {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the running instance rejected the request",
            ))
        }
    }
}

fn serve(pipe: &Handle, stop: &AtomicBool, mut on_request: impl FnMut(Vec<u8>) -> bool) {
    loop {
        // SAFETY: `pipe` is the server instance this thread owns.
        let connected = unsafe { ConnectNamedPipe(pipe.0, None) };
        let connected = match connected {
            Ok(()) => true,
            Err(error) => error.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        if connected && let Some(request) = read_request(pipe) {
            let accepted = request.is_ok_and(&mut on_request);
            let _ = write_all(pipe, &[if accepted { ACCEPTED } else { REJECTED }]);
            // SAFETY: as above.
            let _ = unsafe { FlushFileBuffers(pipe.0) };
        }
        // SAFETY: as above. Disconnecting keeps the instance, so the name stays claimed.
        let _ = unsafe { DisconnectNamedPipe(pipe.0) };
    }
}

/// `None` when the client went away; `Some(Err)` for a request the server refuses.
fn read_request(pipe: &Handle) -> Option<Result<Vec<u8>, ()>> {
    let mut length = [0; 4];
    read_exact(pipe, &mut length).ok()?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_REQUEST_BYTES {
        return Some(Err(()));
    }
    let mut request = vec![0; length];
    read_exact(pipe, &mut request).ok()?;
    Some(Ok(request))
}

fn create_instance(path: &HSTRING) -> io::Result<Handle> {
    let sid = current_user_sid()?;
    let descriptor = SecurityDescriptor::new(&format!("D:P(A;;GA;;;{sid})"))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: `path` and `attributes` (with its descriptor) outlive the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            path,
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            Some(&attributes),
        )
    };
    if pipe.is_invalid() {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(pipe))
}

fn open_client(path: &HSTRING) -> io::Result<Handle> {
    loop {
        // SAFETY: `path` outlives the call. SECURITY_IDENTIFICATION stops the server from acting as
        // this client.
        let result = unsafe {
            CreateFileW(
                path,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
                None,
            )
        };
        match result {
            Ok(handle) => return Ok(Handle(handle)),
            Err(error) if error.code() == ERROR_PIPE_BUSY.to_hresult() => {
                // SAFETY: `path` outlives the call.
                if !unsafe { WaitNamedPipeW(path, CONNECT_WAIT_MS) }.as_bool() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the running instance did not answer",
                    ));
                }
            }
            Err(error) => return Err(os_error(error)),
        }
    }
}

fn write_all(pipe: &Handle, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let mut written = 0;
        // SAFETY: `bytes` is valid for its length; `written` receives the count.
        unsafe { WriteFile(pipe.0, Some(bytes), Some(&mut written), None) }.map_err(os_error)?;
        if written == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn read_exact(pipe: &Handle, mut buffer: &mut [u8]) -> io::Result<()> {
    while !buffer.is_empty() {
        let mut read = 0;
        // SAFETY: `buffer` is valid for its length; `read` receives the count.
        unsafe { ReadFile(pipe.0, Some(buffer), Some(&mut read), None) }.map_err(os_error)?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buffer = &mut buffer[read as usize..];
    }
    Ok(())
}

pub(super) fn current_user_sid() -> io::Result<String> {
    let token = token_user(current_process_token()?)?;
    let sid = token_sid(&token);
    let mut text = PWSTR::null();
    // SAFETY: `sid` points into `token`, which is alive; the call allocates `text`.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(os_error)?;
    // SAFETY: `text` is a NUL-terminated string allocated by the call above.
    let result = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: `text` was allocated with LocalAlloc.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    result
}

fn current_user_sid_bytes() -> io::Result<Vec<u8>> {
    sid_bytes(&token_user(current_process_token()?)?)
}

fn process_user_sid(process_id: u32) -> io::Result<Vec<u8>> {
    // SAFETY: plain Win32 call; the handle is closed by `Handle`.
    let process = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
            .map_err(os_error)?,
    );
    let mut token = HANDLE::default();
    // SAFETY: `process` is open; `token` receives a handle closed by `Handle`.
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }.map_err(os_error)?;
    sid_bytes(&token_user(Handle(token))?)
}

fn current_process_token() -> io::Result<Handle> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle needs no closing; `token` is closed by `Handle`.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(os_error)?;
    Ok(Handle(token))
}

/// The `TOKEN_USER` structure and the SID it points into, in one buffer.
fn token_user(token: Handle) -> io::Result<Vec<u64>> {
    let mut length = 0;
    // SAFETY: the first call only reports the required length.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut length) };
    // u64 elements keep the buffer aligned for TOKEN_USER.
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `length` bytes.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            length,
            &mut length,
        )
    }
    .map_err(os_error)?;
    Ok(buffer)
}

fn token_sid(token_user: &[u64]) -> PSID {
    // SAFETY: `token_user` was filled by GetTokenInformation(TokenUser) and is aligned.
    unsafe { (*token_user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

fn sid_bytes(token_user: &[u64]) -> io::Result<Vec<u8>> {
    let sid = token_sid(token_user);
    // SAFETY: `sid` is a valid SID inside `token_user`.
    let length = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: the SID occupies `length` bytes.
    Ok(unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), length) }.to_vec())
}

fn pipe_path(name: &str) -> HSTRING {
    HSTRING::from(format!(r"\\.\pipe\{name}"))
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the call allocates `descriptor`, freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(os_error)?;
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: allocated with LocalAlloc by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

struct Handle(HANDLE);

// SAFETY: a kernel handle may be used and closed from any thread.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn os_error(error: windows::core::Error) -> io::Error {
    let code = error.code().0 as u32;
    if code & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((code & 0xFFFF) as i32)
    } else {
        io::Error::other(error.message())
    }
}
