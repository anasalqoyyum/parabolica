//! Folder health and explicit removal or relocation.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Role, SharedString, TestSupportExt as _, div, px};
use parabolica_domain::files::RootAvailability;
use parabolica_library::service::Command;

use crate::ui::library::{LibraryView, alert, label};

pub(crate) fn render(view: &LibraryView, cx: &mut Context<LibraryView>) -> AnyElement {
    v_flex().id("music-folders").test_support().gap_3()
        .child(h_flex().gap_3().child(div().text_size(px(22.)).font_semibold().child("Music folders"))
            .child(Button::new("add-music-folder").label("Add music folder").disabled(view.storage.is_none()).on_click(cx.listener(|this,_,_,cx|this.prompt_folder(None,cx))))
            .child(Button::new("folders-rescan").label("Rescan").disabled(view.storage.is_none()).on_click(cx.listener(|this,_,_,cx|this.send(Command::Rescan,cx)))))
        .when_some(view.error.clone().or_else(||view.status.command_error.clone().or_else(||view.status.error.clone())),|v,error|v.child(alert("folders-error",error,cx)))
        .when_some(view.status.watcher_error.clone(),|v,error|v.child(alert("folders-watcher-error",error,cx)))
        .when(view.status.scanning,|v|v.child(h_flex().gap_3().child(label("folders-progress",format!("Indexing · {} files · {} errors",view.status.progress.visited,view.status.progress.errors),cx)).child(Button::new("folders-cancel").label("Cancel indexing").on_click(cx.listener(|this,_,_,_|{if let Some(service)=&this.service{service.cancel();}})))))
        .when(!view.status.failures.is_empty(),|v|v.child(view.render_index_failures(cx)))
        .when(view.roots.is_empty(),|v|v.child(label("folders-empty","No music folders".into(),cx)))
        .children(view.roots.iter().map(|root|{
            let id=root.id;let removing=view.confirm_remove==Some(id);
            let health=match root.availability{RootAvailability::Online=>"Online",RootAvailability::Offline=>"Offline. Reconnect the drive or locate its folder.",RootAvailability::PermissionDenied=>"Permission denied. Restore folder access, then rescan."};
            v_flex().id(SharedString::from(format!("folder-{id}"))).role(Role::Group).aria_label(root.path.to_string_lossy().into_owned()).p_3().gap_2().border_1().border_color(cx.theme().border).rounded(px(8.))
                .child(div().child(root.path.to_string_lossy().into_owned()))
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(health))
                .child(h_flex().gap_2()
                    .child(Button::new(SharedString::from(format!("folder-browse-{id}"))).label("Browse").on_click(cx.listener(move|this,_,_,cx|this.browse_root(id,cx))))
                    .child(Button::new(SharedString::from(format!("folder-locate-{id}"))).label("Locate folder").on_click(cx.listener(move|this,_,_,cx|this.prompt_folder(Some(id),cx))))
                    .child(Button::new(SharedString::from(format!("folder-remove-{id}"))).label("Remove folder").on_click(cx.listener(move|this,_,_,cx|{this.confirm_remove=Some(id);cx.notify();}))))
                .when(removing,|v|v.child(label("remove-folder-consequence","Remove this folder from the collection? Indexed metadata will be removed. Music files stay on disk.".into(),cx))
                    .child(h_flex().gap_2().child(Button::new("confirm-remove-folder").label("Remove from collection").on_click(cx.listener(move|this,_,_,cx|{this.confirm_remove=None;this.send(Command::Remove(id),cx);})))
                        .child(Button::new("cancel-remove-folder").label("Keep folder").on_click(cx.listener(|this,_,_,cx|{this.confirm_remove=None;cx.notify();})))))
        }))
        .into_any_element()
}
