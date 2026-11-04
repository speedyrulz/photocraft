//! One background check at startup, the Install/Ignore offer of a new release, and the window that
//! answers Help › Check for Updates.
//!
//! This fork's releases aren't code-signed, and `craft-update` installs only signed updates, so
//! Install runs the `update.bat` installed beside the app (`services.install_update`) instead.

use photocraft_engine::updates_cmds::{Release, Status};
use serde_json::Value;

use crate::PhotocraftApp;

#[derive(Default)]
pub(crate) struct Runtime {
    startup_checked: bool,
    notified_version: Option<String>,
    /// The check under way was asked for, so it offers even a release the user ignored.
    requested: bool,
    /// Why the last Install didn't start.
    install_error: Option<String>,
}

pub(crate) fn check(app: &mut PhotocraftApp, _ctx: &egui::Context, params: Value) -> Result<Value, String> {
    let before = app.session.updates.status.clone();
    let result = app.run("help.checkForUpdates", params);
    // A check that fails to start still shows its failure and Check Again; the command error
    // stands. Only this call's failure: a Failed left from an earlier check doesn't open the
    // window for a call refused for another reason (bad params).
    let failed_to_start = match (&result, &app.session.updates.status) {
        (Err(error), status @ Status::Failed { message }) => *status != before || (!message.is_empty() && error.contains(message.as_str())),
        _ => false,
    };
    if result.is_ok() || failed_to_start {
        app.ui.updates_open = true;
        app.updates_rt.startup_checked = true;
        // A release the user dismissed or ignored is offered again by a manual check.
        app.updates_rt.notified_version = None;
        app.updates_rt.requested = true;
    }
    result
}

pub(crate) fn tick(app: &mut PhotocraftApp, ctx: &egui::Context) {
    // `updates.status` also polls (automation may consume the result before this frame).
    app.session.updates.poll();
    if let Status::Available { release } = &app.session.updates.status
        && app.updates_rt.notified_version.as_ref() != Some(&release.version)
    {
        app.updates_rt.notified_version = Some(release.version.clone());
        app.ui.updates_open = app.updates_rt.requested || release.version != app.session.prefs().general.ignored_update_version;
    }
    if app.session.updates.check.is_none() {
        return;
    }
    if !app.updates_rt.startup_checked {
        app.updates_rt.startup_checked = true;
        if app.session.prefs().general.automatically_check_updates && matches!(app.session.updates.status, Status::Idle) {
            let _ = app.run("help.checkForUpdates", serde_json::json!({}));
        }
    }
    if matches!(app.session.updates.status, Status::Checking) {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// What the user made of the offer of a new release.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Install,
    /// Never offer this version at launch again; a newer one is offered as usual.
    Ignore,
    /// Closed without choosing: the next launch asks again.
    NotNow,
}

/// Carries out the answer. Install hands over to `services.install_update`, which waits for this
/// app to exit, and then closes through the guarded close path. Builds without an installer (the
/// MSI, a build from source) open the release page instead.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn answer(app: &mut PhotocraftApp, ctx: &egui::Context, release: &Release, answer: Answer) {
    match answer {
        Answer::Install => match app.services.install_update.as_ref().map(|install| install()) {
            None => {
                crate::links::open(app, ctx, &release.url);
                app.ui.updates_open = false;
            }
            Some(Ok(())) => {
                app.ui.updates_open = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(Err(error)) => app.updates_rt.install_error = Some(error),
        },
        Answer::Ignore => {
            app.session.edit_prefs(|p| p.general.ignored_update_version = release.version.clone());
            app.ui.updates_open = false;
        }
        Answer::NotNow => app.ui.updates_open = false,
    }
}

/// The offer of a new release: Install or Ignore.
#[cfg(not(target_arch = "wasm32"))]
fn offer(app: &mut PhotocraftApp, ctx: &egui::Context, release: &Release) {
    let unsaved = app.has_unsaved_work();
    let installs = app.services.install_update.is_some();
    let error = app.updates_rt.install_error.clone();
    let mut chosen = None;
    let mut notes = false;
    let modal = egui::Modal::new(egui::Id::unique("update-offer")).show(ctx, |ui| {
        ui.set_max_width(360.0);
        let title = crate::i18n::fmt(tl!("RulZ Photocraft {version} is available"), &[("version", &release.version)]);
        ui.label(egui::RichText::new(title).font(crate::theme::semibold(15.0)));
        ui.add_space(4.0);
        crate::widgets::hairline(ui);
        ui.add_space(8.0);
        ui.label(crate::i18n::fmt(tl!("You have version {version}."), &[("version", photocraft_engine::build_info::VERSION)]));
        notes = ui.link(tl!("What's new")).clicked();
        if installs && unsaved {
            ui.add_space(4.0);
            ui.label(tl!("Save your documents to install this update."));
        }
        if let Some(error) = &error {
            ui.add_space(4.0);
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let label = if installs { tl!("Install") } else { tl!("Download") };
            let install = ui.add_enabled_ui(!(installs && unsaved), |ui| crate::widgets::primary_button(ui, label, 84.0)).inner;
            if install.clicked() {
                chosen = Some(Answer::Install);
            }
            if crate::widgets::secondary_button(ui, tl!("Ignore"), 84.0).clicked() {
                chosen = Some(Answer::Ignore);
            }
        });
    });
    if notes {
        crate::links::open(app, ctx, &release.url);
    }
    if chosen.is_none() && modal.should_close() {
        chosen = Some(Answer::NotNow);
    }
    if let Some(chosen) = chosen {
        answer(app, ctx, release, chosen);
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn show(_app: &mut PhotocraftApp, _ctx: &egui::Context) {}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.updates_open {
        return;
    }
    if let Status::Available { release } = &app.session.updates.status {
        let release = release.clone();
        offer(app, ctx, &release);
        return;
    }
    let dialog = craft_update::CheckResultDialog::new(&crate::UPDATE_APP, photocraft_engine::build_info::VERSION);
    match dialog.show(ctx, &check_status(&app.session.updates.status)) {
        craft_update::CheckResultResponse::None => {}
        craft_update::CheckResultResponse::Close => app.ui.updates_open = false,
        craft_update::CheckResultResponse::CheckAgain => {
            let _ = check(app, ctx, serde_json::json!({}));
        }
    }
}

/// The engine's check state as the result dialog shows it. An available release goes to the
/// offer, so the dialog draws nothing for it.
#[cfg(not(target_arch = "wasm32"))]
fn check_status(status: &Status) -> craft_update::updater::Status {
    use craft_update::updater::Status as Shown;
    match status {
        Status::Checking => Shown::Checking,
        Status::UpToDate => Shown::UpToDate,
        Status::Failed { message } => Shown::Failed(message.clone()),
        Status::Idle | Status::Available { .. } => Shown::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, menus};
    use photocraft_engine::Session;
    use std::sync::{Arc, Mutex, mpsc};

    type Sender = Arc<Mutex<Option<mpsc::Sender<photocraft_engine::updates_cmds::CheckResult>>>>;

    fn app() -> (PhotocraftApp, Arc<Mutex<u32>>, Sender) {
        let calls = Arc::new(Mutex::new(0));
        let count = calls.clone();
        let sender = Arc::new(Mutex::new(None));
        let tx = sender.clone();
        let app = PhotocraftApp::new(
            Session::new(),
            Services {
                check_updates: Some(Box::new(move || {
                    *count.lock().unwrap() += 1;
                    let (sender, receiver) = mpsc::channel();
                    *tx.lock().unwrap() = Some(sender);
                    Ok(receiver)
                })),
                ..Default::default()
            },
        );
        (app, calls, sender)
    }

    fn at(app: &mut PhotocraftApp, ctx: &egui::Context, time: f64) {
        let mut output = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| tick(app, ui.ctx()));
        output.textures_delta.clear();
    }

    #[test]
    fn automatic_check_runs_once_per_launch_even_after_days_or_preference_changes() {
        let (mut app, calls, sender) = app();
        let ctx = egui::Context::default();
        at(&mut app, &ctx, 0.0);
        at(&mut app, &ctx, 1.0);
        at(&mut app, &ctx, 2.0);
        assert_eq!(*calls.lock().unwrap(), 1);
        sender.lock().unwrap().as_ref().unwrap().send(Ok(None)).unwrap();
        at(&mut app, &ctx, 3.0);
        assert!(!app.ui.updates_open, "automatic up-to-date checks stay quiet");
        at(&mut app, &ctx, 86400.0);
        app.session.edit_prefs(|p| p.general.automatically_check_updates = false);
        at(&mut app, &ctx, 86401.0);
        app.session.edit_prefs(|p| p.general.automatically_check_updates = true);
        at(&mut app, &ctx, 172800.0);
        assert_eq!(*calls.lock().unwrap(), 1, "no periodic checks or preference-triggered rechecks");
        let (mut restarted, restarted_calls, _) = self::app();
        at(&mut restarted, &ctx, 0.0);
        assert_eq!(*restarted_calls.lock().unwrap(), 1, "the next launch checks again");
    }

    #[test]
    fn startup_opt_out_and_failed_checks_do_not_retry_automatically() {
        let (mut disabled, calls, _) = app();
        let ctx = egui::Context::default();
        disabled.session.edit_prefs(|p| p.general.automatically_check_updates = false);
        at(&mut disabled, &ctx, 0.0);
        disabled.session.edit_prefs(|p| p.general.automatically_check_updates = true);
        at(&mut disabled, &ctx, 86400.0);
        assert_eq!(*calls.lock().unwrap(), 0, "opt-out is evaluated at startup");

        let (mut failed, calls, sender) = app();
        at(&mut failed, &ctx, 0.0);
        sender.lock().unwrap().as_ref().unwrap().send(Err("offline".into())).unwrap();
        at(&mut failed, &ctx, 1.0);
        at(&mut failed, &ctx, 172800.0);
        assert_eq!(*calls.lock().unwrap(), 1);
        assert!(!failed.ui.updates_open, "an automatic failure stays quiet");
    }

    #[test]
    fn manual_check_works_when_automatic_checks_are_off_and_opens_status() {
        let (mut app, calls, sender) = app();
        let ctx = egui::Context::default();
        app.session.edit_prefs(|p| p.general.automatically_check_updates = false);
        assert!(menus::is_enabled(&app, "help.checkForUpdates"));
        menus::invoke(&mut app, &ctx, "help.checkForUpdates", serde_json::json!({})).unwrap();
        assert!(app.ui.updates_open);
        assert_eq!(*calls.lock().unwrap(), 1);
        sender.lock().unwrap().as_ref().unwrap().send(Err("offline".into())).unwrap();
        at(&mut app, &ctx, 1.0);
        assert!(matches!(app.session.updates.status, Status::Failed { .. }));
        let mut web = PhotocraftApp::new(Session::new(), Services::default());
        assert!(!menus::is_enabled(&web, "help.checkForUpdates"));
        at(&mut web, &ctx, 2.0);
        assert_eq!(web.session.updates.status, Status::Idle);
    }

    #[test]
    fn available_release_notifies_once_even_when_automation_polls_first() {
        let (mut app, _, sender) = app();
        let ctx = egui::Context::default();
        at(&mut app, &ctx, 0.0);
        sender.lock().unwrap().as_ref().unwrap().send(Ok(Some(found("99.0.0")))).unwrap();
        app.run("updates.status", serde_json::json!({})).unwrap();
        at(&mut app, &ctx, 1.0);
        assert!(app.ui.updates_open);
        app.ui.updates_open = false;
        at(&mut app, &ctx, 2.0);
        assert!(!app.ui.updates_open);
    }

    fn found(version: &str) -> Release {
        Release { version: version.into(), url: format!("https://github.com/speedyrulz/rulz-photocraft/releases/tag/v{version}") }
    }

    /// A launch that finds `version`.
    fn launch(app: &mut PhotocraftApp, sender: &Sender, ctx: &egui::Context, version: &str) {
        at(app, ctx, 0.0);
        sender.lock().unwrap().as_ref().unwrap().send(Ok(Some(found(version)))).unwrap();
        at(app, ctx, 1.0);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn startup_offer_appears_and_renders() {
        let (mut app, _, sender) = app();
        let ctx = egui::Context::default();
        launch(&mut app, &sender, &ctx, "99.0.0");
        assert!(app.ui.updates_open, "startup offer opens on an available release");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui.ctx()));
        output.textures_delta.clear();
        assert!(app.ui.updates_open);
    }

    #[test]
    fn install_hands_over_to_the_installer_and_closes() {
        let (mut app, _, sender) = app();
        let ctx = egui::Context::default();
        let installs = Arc::new(Mutex::new(0));
        let count = installs.clone();
        app.services.install_update = Some(Box::new(move || {
            *count.lock().unwrap() += 1;
            Ok(())
        }));
        launch(&mut app, &sender, &ctx, "99.0.0");
        answer(&mut app, &ctx, &found("99.0.0"), Answer::Install);
        assert_eq!(*installs.lock().unwrap(), 1);
        assert!(!app.ui.updates_open);
    }

    #[test]
    fn an_install_that_fails_to_start_keeps_the_offer_and_says_why() {
        let (mut app, _, sender) = app();
        let ctx = egui::Context::default();
        app.services.install_update = Some(Box::new(|| Err("update.bat is missing".into())));
        launch(&mut app, &sender, &ctx, "99.0.0");
        answer(&mut app, &ctx, &found("99.0.0"), Answer::Install);
        assert!(app.ui.updates_open);
        assert_eq!(app.updates_rt.install_error.as_deref(), Some("update.bat is missing"));
    }

    #[test]
    fn ignored_version_stays_quiet_at_launch_until_a_newer_one_or_a_manual_check() {
        let ctx = egui::Context::default();
        let (mut app, _, sender) = app();
        launch(&mut app, &sender, &ctx, "99.0.0");
        answer(&mut app, &ctx, &found("99.0.0"), Answer::Ignore);
        assert!(!app.ui.updates_open);
        assert_eq!(app.session.prefs().general.ignored_update_version, "99.0.0");
        let ignore = |p: &mut photocraft_engine::prefs::Preferences| p.general.ignored_update_version = "99.0.0".into();

        let (mut next, _, sender) = self::app();
        next.session.edit_prefs(ignore);
        launch(&mut next, &sender, &ctx, "99.0.0");
        assert!(!next.ui.updates_open, "the ignored version isn't offered at launch");

        menus::invoke(&mut next, &ctx, "help.checkForUpdates", serde_json::json!({})).unwrap();
        sender.lock().unwrap().as_ref().unwrap().send(Ok(Some(found("99.0.0")))).unwrap();
        at(&mut next, &ctx, 2.0);
        assert!(next.ui.updates_open, "Help › Check for Updates offers it again");

        let (mut newer, _, sender) = self::app();
        newer.session.edit_prefs(ignore);
        launch(&mut newer, &sender, &ctx, "99.0.1");
        assert!(newer.ui.updates_open, "a newer version is offered");
    }

    #[test]
    fn closing_the_offer_without_choosing_asks_again_next_launch() {
        let (mut app, _, sender) = app();
        let ctx = egui::Context::default();
        launch(&mut app, &sender, &ctx, "99.0.0");
        answer(&mut app, &ctx, &found("99.0.0"), Answer::NotNow);
        assert!(!app.ui.updates_open);
        assert_eq!(app.session.prefs().general.ignored_update_version, "");
    }

    #[test]
    fn a_manual_check_that_fails_to_start_opens_the_status_window() {
        let mut app =
            PhotocraftApp::new(Session::new(), Services { check_updates: Some(Box::new(|| Err("Could not start worker".into()))), ..Default::default() });
        let ctx = egui::Context::default();
        let error = menus::invoke(&mut app, &ctx, "help.checkForUpdates", serde_json::json!({})).unwrap_err();
        assert!(error.contains("Could not start worker"), "{error}");
        assert!(matches!(app.session.updates.status, Status::Failed { .. }));
        assert!(app.ui.updates_open, "the failure and Check Again are shown");
        // Bad params are a caller error, not a failed check: nothing opens, also while the earlier
        // failure is still the status.
        app.ui.updates_open = false;
        assert!(menus::invoke(&mut app, &ctx, "help.checkForUpdates", serde_json::json!({"x": 1})).is_err());
        assert!(matches!(app.session.updates.status, Status::Failed { .. }), "the earlier failure stays");
        assert!(!app.ui.updates_open, "bad params after a failed check");
        app.session.updates.status = Status::Idle;
        assert!(menus::invoke(&mut app, &ctx, "help.checkForUpdates", serde_json::json!({"x": 1})).is_err());
        assert!(!app.ui.updates_open);
        // The same start failure again opens it again.
        app.session.updates.status = Status::Failed { message: "Could not start worker".into() };
        assert!(menus::invoke(&mut app, &ctx, "help.checkForUpdates", serde_json::json!({})).is_err());
        assert!(app.ui.updates_open, "a repeated start failure");
    }

    #[test]
    fn update_window_is_drivable_and_bad_state_changes_are_atomic() {
        let (mut app, _, _) = app();
        let ctx = egui::Context::default();
        let call = |app: &mut PhotocraftApp, method: &str, params: Value| {
            let (req, _) = crate::control::ControlRequest::new(method, params);
            match crate::control::handle(app, &ctx, &req) {
                crate::control::Outcome::Done(value) => value,
                _ => panic!("expected an immediate reply"),
            }
        };
        assert_eq!(call(&mut app, "ui.set", serde_json::json!({"updatesOpen": true}))["ok"], true);
        let inspected = call(&mut app, "ui.inspect", serde_json::json!({}));
        assert_eq!(inspected["result"]["updatesOpen"], true);
        assert_eq!(inspected["result"]["updates"]["supported"], true);
        assert_eq!(call(&mut app, "ui.set", serde_json::json!({"updatesOpen": false, "maskTarget": "bad"}))["ok"], false);
        assert!(app.ui.updates_open);
        assert_eq!(call(&mut app, "ui.set", serde_json::json!({"updatesOpen": false}))["ok"], true);
        assert!(!app.ui.updates_open);
    }
}
