mod connection;
mod general;
mod logs;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};

use super::{SettingsField, SettingsTab};
use crate::{
    ProxyBear,
    app::{presentation::format_bytes, stats::StatsSnapshot},
};

pub struct SettingsView {
    app: Entity<ProxyBear>,
    view: WeakEntity<Self>,
    server: Entity<InputState>,
    username: Entity<InputState>,
    port: Entity<InputState>,
    pool_size: Entity<InputState>,
    key_path: Entity<InputState>,
    key_password: Entity<InputState>,
    ssh_password: Entity<InputState>,
    local_addr: Entity<InputState>,
    log_query: Entity<InputState>,
    log_filter: logs::LineFilter,
    log_scroll: ScrollHandle,
    log_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(app: Entity<ProxyBear>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = app.read(cx).form.clone();
        let log_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let mut subscriptions = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe(&log_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let mut input = |value: String,
                         placeholder: &'static str,
                         masked: bool,
                         field: fn(String) -> SettingsField| {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
                    .masked(masked)
            });
            let app = app.clone();
            subscriptions.push(cx.subscribe(&state, move |_, state, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = state.read(cx).value().to_string();
                    app.update(cx, |app, cx| app.handle_field(field(value), cx));
                }
            }));
            state
        };
        Self {
            server: input(
                form.server,
                "host.example.com",
                false,
                SettingsField::Server,
            ),
            username: input(form.username, "user", false, SettingsField::Username),
            port: input(form.port, "22", false, SettingsField::Port),
            pool_size: input(form.pool_size, "3", false, SettingsField::PoolSize),
            key_path: input(
                form.key_path,
                "~/.ssh/id_ed25519",
                false,
                SettingsField::KeyPath,
            ),
            key_password: input(form.key_password, "None", true, SettingsField::KeyPassword),
            ssh_password: input(
                form.ssh_password,
                "Required",
                true,
                SettingsField::SshPassword,
            ),
            local_addr: input(
                form.local_addr,
                "127.0.0.1:1080",
                false,
                SettingsField::LocalAddr,
            ),
            log_query,
            log_filter: logs::LineFilter::default(),
            app,
            view: cx.entity().downgrade(),
            log_scroll: ScrollHandle::new(),
            log_focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Copy form values the app changed on its own (file picker, revert)
    /// into the inputs.
    fn sync_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let form = self.app.read(cx).form.clone();
        for (input, value) in [
            (&self.server, form.server),
            (&self.username, form.username),
            (&self.port, form.port),
            (&self.pool_size, form.pool_size),
            (&self.key_path, form.key_path),
            (&self.key_password, form.key_password),
            (&self.ssh_password, form.ssh_password),
            (&self.local_addr, form.local_addr),
        ] {
            input.update(cx, |input, cx| {
                if input.value() != value {
                    input.set_value(value, window, cx);
                }
            });
        }
    }

    fn on_field(
        &self,
        field: SettingsField,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<> {
        let app = self.app.clone();
        let view = self.view.clone();
        let scroll = self.log_scroll.clone();
        move |_, window, cx| {
            let sync = matches!(field, SettingsField::ChooseKey | SettingsField::Revert);
            let reset_logs = matches!(
                field,
                SettingsField::Tab(SettingsTab::Logs) | SettingsField::ClearLog
            );
            app.update(cx, |app, cx| app.handle_field(field.clone(), cx));
            if sync {
                let _ = view.update(cx, |view, cx| view.sync_inputs(window, cx));
            }
            if reset_logs {
                scroll.set_offset(point(px(0.), px(0.)));
            }
        }
    }

    /// A click handler that changes view-only state.
    fn on_view(
        &self,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let view = self.view.clone();
        move |_, _, cx| {
            let _ = view.update(cx, |view, cx| {
                f(view, cx);
                cx.notify();
            });
        }
    }

    fn button(&self, id: &'static str, label: &'static str, field: SettingsField) -> Button {
        Button::new(id).label(label).on_click(self.on_field(field))
    }

    fn choice_button(
        &self,
        id: &'static str,
        label: &'static str,
        field: SettingsField,
        active: bool,
    ) -> Button {
        self.button(id, label, field)
            .small()
            .selected(active)
            .toggled(active)
            .when(active, |button| button.primary())
    }

    fn switch(&self, id: &'static str, checked: bool, field: fn(bool) -> SettingsField) -> Switch {
        let app = self.app.clone();
        Switch::new(id)
            .checked(checked)
            .on_click(move |checked, _, cx| {
                app.update(cx, |app, cx| app.handle_field(field(*checked), cx));
            })
    }

    fn nav_item(
        &self,
        id: &'static str,
        icon: IconName,
        label: &'static str,
        tab: SettingsTab,
        cx: &App,
    ) -> impl IntoElement {
        let active = self.app.read(cx).active_tab == tab;
        h_flex()
            .id(id)
            .gap_2p5()
            .px_3()
            .py_2()
            .rounded_md()
            .text_size(px(13.))
            .cursor_pointer()
            .when(active, |item| {
                item.bg(cx.theme().accent)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().accent_foreground)
            })
            .when(!active, |item| {
                item.hover(|item| item.bg(cx.theme().secondary_hover))
            })
            .child(Icon::new(icon).size_4())
            .child(label)
            .on_click(self.on_field(SettingsField::Tab(tab)))
    }

    fn sidebar(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let stats = &app.stats_snapshot;
        let state = ConnectionState::of(stats, app.proxy.is_running());
        v_flex()
            .w(px(180.))
            .h_full()
            .flex_shrink_0()
            .p_3()
            .gap_0p5()
            .bg(cx.theme().muted)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_4()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("ProxyBear"),
            )
            .child(self.nav_item(
                "nav-general",
                IconName::LayoutDashboard,
                "General",
                SettingsTab::General,
                cx,
            ))
            .child(self.nav_item(
                "nav-connection",
                IconName::Network,
                "Connection",
                SettingsTab::Connection,
                cx,
            ))
            .child(self.nav_item(
                "nav-logs",
                IconName::SquareTerminal,
                "Logs",
                SettingsTab::Logs,
                cx,
            ))
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap_1()
                    .px_2()
                    .py_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_size(px(13.))
                            .child(div().size_2().rounded_full().bg(state.color(cx)))
                            .child(state.label()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(traffic(stats)),
                    ),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.app.read(cx).active_tab {
            SettingsTab::General => self.general(cx).into_any_element(),
            SettingsTab::Connection => self.connection(cx).into_any_element(),
            SettingsTab::Logs => self.logs(cx).into_any_element(),
        };
        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.sidebar(cx))
            .child(v_flex().flex_1().min_w_0().h_full().child(page))
    }
}

/// What the proxy is doing, as shown to the user.
#[derive(Clone, Copy)]
enum ConnectionState {
    Stopped,
    Connecting,
    Connected,
    Failed,
}

impl ConnectionState {
    fn of(stats: &StatsSnapshot, running: bool) -> Self {
        if stats.last_error.is_some() {
            Self::Failed
        } else if !running {
            Self::Stopped
        } else if stats.ssh_connected {
            Self::Connected
        } else {
            Self::Connecting
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Connecting => "Connecting…",
            Self::Connected => "Connected",
            Self::Failed => "Error",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Stopped => IconName::Pause,
            Self::Connecting => IconName::LoaderCircle,
            Self::Connected => IconName::CircleCheck,
            Self::Failed => IconName::TriangleAlert,
        }
    }

    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Stopped => cx.theme().muted_foreground,
            Self::Connecting => cx.theme().warning,
            Self::Connected => cx.theme().success,
            Self::Failed => cx.theme().danger,
        }
    }
}

fn traffic(stats: &StatsSnapshot) -> String {
    format!(
        "↑ {}   ↓ {}",
        format_bytes(stats.bytes_up),
        format_bytes(stats.bytes_down)
    )
}

fn page_header(
    title: &'static str,
    description: impl Into<SharedString>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(description.into()),
        )
}

/// A rounded box of rows separated by hairlines, with an optional heading.
fn group(
    title: Option<&'static str>,
    rows: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .when_some(title, |group, title| {
            group.child(
                div()
                    .px_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(title),
            )
        })
        .child(
            v_flex()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .children(rows.into_iter().enumerate().map(|(ix, row)| {
                    div()
                        .when(ix > 0, |row| {
                            row.border_t_1().border_color(cx.theme().border)
                        })
                        .child(row)
                })),
        )
}

/// A settings row: label on the left, control on the right.
fn row(label: &'static str, control: impl IntoElement, cx: &App) -> AnyElement {
    row_inner(label, None, control, cx)
}

/// A settings row with help text under the label.
fn row_with(
    label: &'static str,
    description: impl IntoElement,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    row_inner(label, Some(description.into_any_element()), control, cx)
}

fn row_inner(
    label: &'static str,
    description: Option<AnyElement>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .gap_4()
        .px_4()
        .py_2p5()
        .min_h(px(44.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                )
                .when_some(description, |label, description| {
                    label.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                }),
        )
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// A small text input of fixed width, for use as a row control.
fn input(state: &Entity<InputState>, width: f32) -> impl IntoElement {
    div().w(px(width)).child(Input::new(state).small())
}

/// Read-only text shown as a row's value.
fn value(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .max_w(px(280.))
        .truncate()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
