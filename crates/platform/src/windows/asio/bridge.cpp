#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <objbase.h>
#include <cstdint>
#include <new>
#include "iasiodrv.h"

static_assert(sizeof(long) == 4 && sizeof(ASIOBool) == 4);
static_assert(sizeof(ASIOBufferInfo) == (sizeof(void*) == 8 ? 24 : 16));

struct Driver {
    IASIO* asio = nullptr;
    bool buffers = false;
    bool started = false;
};

extern "C" {
int32_t parabolica_asio_open(const wchar_t* clsid, Driver** result) {
    *result = nullptr;
    HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) return hr;
    CLSID id;
    hr = CLSIDFromString(clsid, &id);
    IASIO* asio = nullptr;
    if (SUCCEEDED(hr)) hr = CoCreateInstance(id, nullptr, CLSCTX_INPROC_SERVER, id, reinterpret_cast<void**>(&asio));
    if (FAILED(hr)) { CoUninitialize(); return hr; }
    // A null system reference is permitted by ASIO. Do not parent panels to another app.
    if (!asio->init(nullptr)) { asio->Release(); CoUninitialize(); return ASE_NotPresent; }
    Driver* driver = new (std::nothrow) Driver;
    if (!driver) { asio->Release(); CoUninitialize(); return ASE_NoMemory; }
    driver->asio = asio;
    *result = driver;
    return ASE_OK;
}
void parabolica_asio_close(Driver* driver) {
    if (driver->started) driver->asio->stop();
    if (driver->buffers) driver->asio->disposeBuffers();
    driver->asio->Release();
    delete driver;
    CoUninitialize();
}
void parabolica_asio_name(Driver* driver, char* name) { driver->asio->getDriverName(name); }
int32_t parabolica_asio_version(Driver* driver) { return driver->asio->getDriverVersion(); }
int32_t parabolica_asio_channels(Driver* driver, int32_t* outputs) {
    long inputs = 0, count = 0;
    const auto error = driver->asio->getChannels(&inputs, &count);
    *outputs = count;
    return error;
}
int32_t parabolica_asio_channel(Driver* driver, int32_t index, int32_t* type, char* name) {
    ASIOChannelInfo info{};
    info.channel = index;
    const auto error = driver->asio->getChannelInfo(&info);
    *type = info.type;
    memcpy(name, info.name, sizeof(info.name));
    return error;
}
int32_t parabolica_asio_buffers(Driver* driver, int32_t* sizes) {
    long minimum = 0, maximum = 0, preferred = 0, granularity = 0;
    const auto error = driver->asio->getBufferSize(&minimum, &maximum, &preferred, &granularity);
    sizes[0] = minimum; sizes[1] = maximum; sizes[2] = preferred; sizes[3] = granularity;
    return error;
}
int32_t parabolica_asio_can_rate(Driver* driver, double rate) { return driver->asio->canSampleRate(rate); }
int32_t parabolica_asio_rate(Driver* driver, double* rate) { return driver->asio->getSampleRate(rate); }
int32_t parabolica_asio_set_rate(Driver* driver, double rate) { return driver->asio->setSampleRate(rate); }
int32_t parabolica_asio_latency(Driver* driver, int32_t* frames) {
    long input = 0, output = 0;
    const auto error = driver->asio->getLatencies(&input, &output);
    *frames = output;
    return error;
}
int32_t parabolica_asio_create(Driver* driver, ASIOBufferInfo* buffers, int32_t channels, int32_t frames, ASIOCallbacks* callbacks) {
    const auto error = driver->asio->createBuffers(buffers, channels, frames, callbacks);
    driver->buffers = error == ASE_OK;
    return error;
}
int32_t parabolica_asio_start(Driver* driver) {
    const auto error = driver->asio->start();
    driver->started = error == ASE_OK;
    return error;
}
int32_t parabolica_asio_panel(Driver* driver) { return driver->asio->controlPanel(); }
void parabolica_asio_pump() {
    MSG message;
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}
}
