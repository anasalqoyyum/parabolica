use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, Role, SharedString, TestSupportExt as _, Window, div, px};
use parabolica_provider_contracts::account::{AccountProblem, Connection};
use parabolica_provider_contracts::auth::AuthorizationError;
use parabolica_provider_contracts::error::{ProviderError, ProviderErrorKind};
use parabolica_provider_contracts::service::ServiceId;

use super::AccountView;

/// Account cards in Settings › Sources & output.
#[derive(IntoElement)]
pub struct AccountsSettings;

impl RenderOnce for AccountsSettings {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .id("accounts")
            .gap_4()
            .children(super::views(cx).into_iter().map(|view| card(view, cx)))
    }
}

fn card(view: AccountView, cx: &App) -> impl IntoElement {
    let service = view.status.service;
    let key = service.key();
    let id = |part: &str| SharedString::from(format!("account-{key}-{part}"));
    let theme = cx.theme();
    let (icon, state, color) = state(&view, cx);
    let account_name = view
        .status
        .connection
        .account()
        .map(|account| account.display_name.clone());
    let summary: SharedString = match &account_name {
        Some(name) => format!("{name} · {state}").into(),
        None => state.into(),
    };

    v_flex()
        .id(id("card"))
        .test_support()
        .role(Role::Group)
        .aria_label(service.name())
        .p_5()
        .gap_3()
        .rounded(px(12.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .child(
            h_flex()
                .gap_3()
                .child(
                    div()
                        .id(id("title"))
                        .role(Role::Heading)
                        .aria_level(2)
                        .aria_label(service.name())
                        .text_size(px(22.))
                        .font_semibold()
                        .child(service.name()),
                )
                .when(service == ServiceId::YOUTUBE_MUSIC, |this| {
                    this.child(Tag::secondary().outline().small().child("Experimental"))
                })
                .child(div().flex_1())
                .children(actions(&view)),
        )
        .child(
            h_flex()
                .id(id("status"))
                .test_support()
                .role(Role::Label)
                .aria_value(summary)
                .gap_2()
                .text_color(theme.muted_foreground)
                .child(Icon::new(icon).text_color(color))
                .when_some(account_name, |this, name| {
                    this.child(div().text_color(theme.foreground).child(name))
                        .child("·")
                })
                .child(div().text_color(color).child(state)),
        )
        .when(!view.can_sign_in && !view.status.signing_in, |this| {
            const TEXT: &str = "Sign-in is not available in this build.";
            this.child(
                div()
                    .id(id("sign-in-unavailable"))
                    .test_support()
                    .role(Role::Label)
                    .aria_value(TEXT)
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(TEXT),
            )
        })
        .when_some(view.status.problem.as_ref().map(problem), |this, text| {
            this.child(
                h_flex()
                    .id(id("problem"))
                    .test_support()
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .gap_2()
                    .text_sm()
                    .text_color(theme.warning)
                    .child(Icon::new(IconName::TriangleAlert))
                    .child(text),
            )
        })
}

fn state(view: &AccountView, cx: &App) -> (IconName, &'static str, Hsla) {
    let theme = cx.theme();
    if view.status.signing_in {
        return (
            IconName::LoaderCircle,
            "Continue in your browser",
            theme.foreground,
        );
    }
    match view.status.connection {
        Connection::Connected(_) => (IconName::CircleCheck, "Connected", theme.success),
        Connection::Expired(_) => (IconName::TriangleAlert, "Session expired", theme.warning),
        Connection::SignedOut => (
            IconName::CircleUser,
            "Not connected",
            theme.muted_foreground,
        ),
    }
}

fn actions(view: &AccountView) -> Vec<Button> {
    let service = view.status.service;
    let key = service.key();
    let id = |part: &str| SharedString::from(format!("account-{key}-{part}"));

    if view.status.signing_in {
        return vec![
            Button::new(id("cancel"))
                .outline()
                .label("Cancel")
                .on_click(move |_, _, cx| super::cancel_sign_in(service, cx)),
        ];
    }
    let connect = |label: &'static str| {
        Button::new(id("connect"))
            .primary()
            .label(label)
            .loading(view.connecting)
            .disabled(!view.can_sign_in || view.connecting || view.disconnecting)
            .on_click(move |_, _, cx| super::connect(service, cx))
    };
    let disconnect = Button::new(id("disconnect"))
        .outline()
        .label("Disconnect")
        .loading(view.disconnecting)
        .disabled(view.connecting || view.disconnecting)
        .on_click(move |_, _, cx| super::disconnect(service, cx));
    match view.status.connection {
        Connection::SignedOut => vec![connect("Connect")],
        Connection::Expired(_) => vec![connect("Reconnect"), disconnect],
        Connection::Connected(_) => vec![disconnect],
    }
}

fn problem(problem: &AccountProblem) -> SharedString {
    match problem {
        AccountProblem::Restore(error) => {
            format!("The saved sign-in could not be read: {}.", error.kind)
        }
        AccountProblem::SignIn(ProviderError {
            kind: ProviderErrorKind::Authorization(AuthorizationError::WrongAccount),
            ..
        }) => "Sign-in used a different account. Disconnect first to switch accounts.".to_owned(),
        AccountProblem::SignIn(error) => format!("Sign-in failed: {}.", error.kind),
        AccountProblem::Remove(error) => format!(
            "Still connected. The saved sign-in could not be removed: {}.",
            error.kind
        ),
        AccountProblem::Revoke(error) => format!(
            "Signed out on this PC. {} did not confirm revoking access: {}.",
            error.service, error.kind
        ),
    }
    .into()
}
