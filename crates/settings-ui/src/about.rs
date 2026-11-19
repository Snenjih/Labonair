// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! The About card at the top of Settings › General: identity, version, update
//! check, project links and the licence / attribution notice (GPL-3.0-or-later
//! with the §7 terms in `ADDITIONAL-TERMS.md`).
//!
//! Part of `SettingsView` — see `crate::view`.

use crate::view::*;

const WEBSITE: &str = "https://labonair.dev";
const REPOSITORY: &str = "https://github.com/Snenjih/Labonair";
const LICENSE_URL: &str = "https://github.com/Snenjih/Labonair/blob/master/LICENSE";
const ADDITIONAL_TERMS_URL: &str =
    "https://github.com/Snenjih/Labonair/blob/master/ADDITIONAL-TERMS.md";

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

/// Percent-encode for a URL query value (RFC 3986 unreserved set kept).
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A pre-filled "new issue" URL carrying version, OS and architecture.
fn report_problem_url(version: &str) -> String {
    let body = format!(
        "**Version:** {version}\n**OS:** {} ({})\n\n**What happened?**\n\n**Expected behaviour**\n\n**Steps to reproduce**\n",
        os_name(),
        std::env::consts::ARCH,
    );
    format!("{REPOSITORY}/issues/new?body={}", encode(&body))
}

impl SettingsView {
    pub(crate) fn render_about(&self, c: &Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let about = self.services.about();
        let version = about.map_or_else(
            || env!("CARGO_PKG_VERSION").to_owned(),
            |a| a.version.clone(),
        );
        let meta = format!(
            "{} \u{b7} {} \u{b7} v{version}",
            os_name(),
            std::env::consts::ARCH
        );

        let link = |id: &'static str,
                    icon: IconName,
                    title: &'static str,
                    subtitle: &'static str,
                    url: String| {
            ListItem::new(id, *c)
                .leading(icon.svg(c.muted).size(gpui::px(20.0)))
                .child(
                    v_stack()
                        .py(c.space(4.0))
                        .child(
                            div()
                                .text_size(c.control_space(14.0))
                                .text_color(c.fg)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(c.control_space(12.0))
                                .text_color(c.muted)
                                .child(subtitle),
                        ),
                )
                .on_click(move |_, _, cx| cx.open_url(&url))
        };

        let mut identity = v_stack().gap(c.space(12.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(c.space(16.0))
                .child(
                    gpui::img("brand/labonair.png")
                        .size(gpui::px(64.0))
                        .rounded(c.space(12.0)),
                )
                .child(
                    v_stack()
                        .child(
                            div()
                                .text_size(c.control_space(24.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(c.fg)
                                .child("Labonair"),
                        )
                        .child(
                            div()
                                .font_family("monospace")
                                .text_size(c.control_space(12.0))
                                .text_color(c.muted)
                                .child(SharedString::from(meta)),
                        ),
                ),
        );
        if about.is_some() {
            identity = identity.child(
                div().child(
                    button(
                        "about-check-updates",
                        *c,
                        ButtonVariant::Action,
                        ButtonSize::Sm,
                    )
                    .child("Check for updates")
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        if let Some(about) = this.services.about().cloned() {
                            about.check_for_updates(cx);
                        }
                    })),
                ),
            );
        }

        let license_link = |id: &'static str, label: &'static str, url: &'static str| {
            let url = url.to_owned();
            button(id, *c, ButtonVariant::Link, ButtonSize::None)
                .child(label)
                .on_click(move |_, _, cx| cx.open_url(&url))
        };
        let notice = |text: &'static str| {
            div()
                .text_size(c.control_space(12.0))
                .text_color(c.muted)
                .child(text)
        };
        let legal = v_stack()
            .gap(c.space(2.0))
            .child(notice(
                "Copyright \u{a9} 2026 Snenjih. GPL-3.0-or-later. No warranty; redistribution permitted under this license.",
            ))
            .child(notice("Based on Labonair by Snenjih \u{2014} labonair.dev"))
            .child(
                h_stack()
                    .items_center()
                    .gap(c.space(8.0))
                    .child(license_link("about-license", "License", LICENSE_URL))
                    .child(license_link(
                        "about-additional-terms",
                        "Additional terms",
                        ADDITIONAL_TERMS_URL,
                    )),
            );
        identity = identity.child(legal);

        let links = v_stack()
            .gap(c.space(4.0))
            .child(link(
                "about-report",
                IconName::Warning,
                "Report a problem",
                "Generate a pre-filled GitHub issue",
                report_problem_url(&version),
            ))
            .child(link(
                "about-github",
                IconName::Github,
                "GitHub",
                "Source code",
                REPOSITORY.to_owned(),
            ))
            .child(link(
                "about-website",
                IconName::Globe,
                "Website",
                "labonair.dev",
                WEBSITE.to_owned(),
            ));

        v_stack()
            .w_full()
            .p(c.space(16.0))
            .gap(c.space(8.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_start()
                    .child(identity)
                    .child(links),
            )
            .into_any_element()
    }
}
