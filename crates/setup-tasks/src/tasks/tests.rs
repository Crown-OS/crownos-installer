use crownos_config::system::{
    Browser, DeveloperTools, SETUP_STATE_DIR, SetupConfig, SetupTask, TaskState, TaskStatus,
};

use super::{Context, PassSummary, run_pass};
use crate::context::fake::FakeSystem;
use crate::state::StateStore;

const GENERATION: u64 = 42;
const MIMEAPPS: &str = "/etc/xdg/mimeapps.list";
const PLAYWRIGHT_BIN: &str = "/usr/lib/crownos/playwright/node_modules/.bin";

fn config(browser: Browser, developer_tools: DeveloperTools) -> SetupConfig {
    SetupConfig {
        browser,
        developer_tools,
    }
}

fn claude_code_only() -> DeveloperTools {
    DeveloperTools {
        claude_code: true,
        ..DeveloperTools::default()
    }
}

fn first_boot() -> FakeSystem {
    FakeSystem::new().with_installed(&["chromium"])
}

fn with_playwright_ready(system: FakeSystem) -> FakeSystem {
    system
        .with_file("/usr/bin/node", "")
        .with_file("/usr/bin/npm", "")
        .with_file(format!("{PLAYWRIGHT_BIN}/playwright-mcp"), "")
        .with_file(format!("{PLAYWRIGHT_BIN}/playwright-cli"), "")
        .with_link(
            "/usr/local/bin/playwright-cli",
            format!("{PLAYWRIGHT_BIN}/playwright-cli"),
        )
        .with_directory("/usr/share/playwright/firefox-1495")
}

fn pass(system: &FakeSystem, config: SetupConfig, online: bool) -> PassSummary {
    system.set_connectivity(&[online]);
    let context = Context::new(config, system);
    run_pass(
        &context,
        &StateStore::new(system, SETUP_STATE_DIR),
        GENERATION,
    )
    .expect("pass runs")
}

fn status(system: &FakeSystem, task: SetupTask) -> Option<TaskStatus> {
    StateStore::new(system, SETUP_STATE_DIR)
        .load(task)
        .expect("state loads")
        .map(|state| state.status)
}

fn is_state_entry(entry: &str) -> bool {
    entry.ends_with(SETUP_STATE_DIR)
        || SetupTask::ALL
            .iter()
            .any(|task| entry.ends_with(&format!("{SETUP_STATE_DIR}/{}.ron", task.id())))
}

fn only_queries_ran(system: &FakeSystem) -> bool {
    system
        .journal()
        .iter()
        .all(|entry| entry.starts_with("run pacman -Qq -- ") || entry.starts_with("connect "))
}

#[test]
fn offline_first_boot_sets_the_bundled_browser_and_waits_for_playwright() {
    let system = first_boot();
    let summary = pass(&system, SetupConfig::default(), false);

    assert_eq!(
        summary,
        PassSummary {
            failed: Vec::new(),
            waiting_for_network: vec![SetupTask::Playwright],
            given_up: Vec::new(),
        }
    );
    assert_eq!(status(&system, SetupTask::Browser), Some(TaskStatus::Done));
    assert_eq!(status(&system, SetupTask::DeveloperTools), None);
    assert_eq!(
        status(&system, SetupTask::Playwright),
        Some(TaskStatus::Pending)
    );
    assert!(
        system
            .file(MIMEAPPS)
            .is_some_and(|text| text.contains("text/html=chromium.desktop"))
    );
    assert!(
        !system
            .commands()
            .iter()
            .any(|command| command.starts_with("pacman -S") || command.starts_with("npm"))
    );

    system.clear_journal();
    pass(&system, SetupConfig::default(), false);
    assert!(only_queries_ran(&system), "{:?}", system.journal());
}

#[test]
fn a_second_pass_issues_no_command_when_everything_is_done() {
    let system = with_playwright_ready(first_boot());
    let config = config(Browser::Firefox, claude_code_only());

    let summary = pass(&system, config, true);
    assert!(summary.is_complete(), "{summary:?}");
    let commands = system.commands();
    assert!(commands.contains(&"pacman -Syu --needed --noconfirm -- firefox".to_owned()));
    assert!(commands.contains(&"pacman -Syu --needed --noconfirm -- claude-code".to_owned()));
    assert!(system.file(MIMEAPPS).is_some_and(
        |text| text.contains("text/html=firefox.desktop") && !text.contains("chromium")
    ));
    for task in SetupTask::ALL {
        assert_eq!(status(&system, task), Some(TaskStatus::Done), "{task}");
    }

    system.clear_journal();
    assert!(pass(&system, config, true).is_complete());
    assert!(only_queries_ran(&system), "{:?}", system.journal());
}

#[test]
fn a_failing_install_is_recorded_and_later_tasks_still_run() {
    let system = with_playwright_ready(first_boot()).with_failure(
        "pacman -Syu --needed --noconfirm -- brave-bin",
        "error: target not found: brave-bin\n",
    );
    let summary = pass(&system, config(Browser::Brave, claude_code_only()), true);

    assert_eq!(summary.failed.len(), 1);
    assert_eq!(summary.failed[0].0, SetupTask::Browser);
    let Some(TaskStatus::Failed { message }) = status(&system, SetupTask::Browser) else {
        panic!("the browser task did not fail");
    };
    assert!(message.contains("target not found: brave-bin"), "{message}");
    assert_eq!(
        status(&system, SetupTask::DeveloperTools),
        Some(TaskStatus::Done)
    );
    assert_eq!(
        status(&system, SetupTask::Playwright),
        Some(TaskStatus::Done)
    );
    assert_eq!(system.file(MIMEAPPS), None);
}

#[test]
fn an_unwanted_task_loses_its_state_file() {
    let system = with_playwright_ready(first_boot());
    StateStore::new(&system, SETUP_STATE_DIR)
        .reset(&[SetupTask::DeveloperTools], 1)
        .expect("resets");

    pass(&system, SetupConfig::default(), true);

    assert_eq!(status(&system, SetupTask::DeveloperTools), None);
    assert!(
        system
            .journal()
            .contains(&format!("remove {SETUP_STATE_DIR}/developer-tools.ron"))
    );
}

#[test]
fn a_done_task_records_the_new_generation() {
    let system = with_playwright_ready(first_boot());
    StateStore::new(&system, SETUP_STATE_DIR)
        .save(
            SetupTask::Playwright,
            &TaskState {
                status: TaskStatus::Done,
                generation: 1,
                started_at_unix: None,
                finished_at_unix: None,
                attempts: 0,
            },
        )
        .expect("saves");

    pass(&system, SetupConfig::default(), true);

    let state = StateStore::new(&system, SETUP_STATE_DIR)
        .load(SetupTask::Playwright)
        .expect("loads")
        .expect("exists");
    assert_eq!(state.generation, GENERATION);
}

const NODE_INSTALL: &str = "pacman -Syu --needed --noconfirm -- nodejs npm";
const NPM_INSTALL: &str = "npm install --prefix /usr/lib/crownos/playwright --cache /var/lib/crownos/setup/npm-cache --no-fund --no-audit @playwright/cli@0.1.25 @playwright/mcp@0.0.86";

fn firefox_download() -> String {
    format!(
        "env PLAYWRIGHT_BROWSERS_PATH=/usr/share/playwright {PLAYWRIGHT_BIN}/playwright-cli install-browser firefox"
    )
}

fn with_playwright_installers(system: FakeSystem) -> FakeSystem {
    system
        .with_command_writes(NODE_INSTALL, "/usr/bin/node", "")
        .with_command_writes(NODE_INSTALL, "/usr/bin/npm", "")
        .with_command_writes(NPM_INSTALL, format!("{PLAYWRIGHT_BIN}/playwright-cli"), "")
        .with_command_writes(NPM_INSTALL, format!("{PLAYWRIGHT_BIN}/playwright-mcp"), "")
        .with_command_writes(
            &firefox_download(),
            "/usr/share/playwright/firefox-1578",
            "",
        )
}

#[test]
fn playwright_installs_node_the_packages_and_the_firefox_build() {
    let system = with_playwright_installers(first_boot().with_installed(&["firefox"]));
    let config = config(Browser::Firefox, DeveloperTools::default());
    assert!(pass(&system, config, true).is_complete());

    let journal = system.journal();
    let position = |entry: &str| {
        journal
            .iter()
            .position(|recorded| recorded == entry)
            .unwrap_or_else(|| panic!("{entry} missing from {journal:#?}"))
    };
    let node = position(&format!("run {NODE_INSTALL}"));
    let npm = position(&format!("run {NPM_INSTALL}"));
    let link = position(&format!(
        "symlink /usr/local/bin/playwright-cli -> {PLAYWRIGHT_BIN}/playwright-cli"
    ));
    let firefox = position(&format!("run {}", firefox_download()));
    assert!(node < npm && npm < link && link < firefox);
    assert_eq!(
        status(&system, SetupTask::Playwright),
        Some(TaskStatus::Done)
    );

    system.clear_journal();
    assert!(pass(&system, config, true).is_complete());
    assert!(only_queries_ran(&system), "{:?}", system.journal());
}

#[test]
fn a_run_that_leaves_the_task_incomplete_fails() {
    let system = first_boot()
        .with_installed(&["firefox"])
        .with_program("npm")
        .with_program("env");
    let summary = pass(
        &system,
        config(Browser::Firefox, DeveloperTools::default()),
        true,
    );

    assert_eq!(
        summary.failed,
        [(
            SetupTask::Playwright,
            "ran, but the result is not complete".to_owned()
        )]
    );
}

const BUILD_DIRECTORY: &str = "/var/lib/crownos/setup/build/google-chrome";

fn with_chrome_build(system: FakeSystem) -> FakeSystem {
    let makepkg = format!(
        "as crownos-build in {BUILD_DIRECTORY}: HOME={BUILD_DIRECTORY} makepkg --nodeps --nocheck --noconfirm --force"
    );
    let clone = format!(
        "as crownos-build in {BUILD_DIRECTORY}: HOME={BUILD_DIRECTORY} git clone https://aur.archlinux.org/google-chrome.git {BUILD_DIRECTORY}"
    );
    with_playwright_ready(system)
        .with_program("id")
        .with_command_writes(
            &clone,
            format!("{BUILD_DIRECTORY}/.SRCINFO"),
            "pkgbase = google-chrome\n\tdepends = gtk3\n\tdepends = nss>=3.26\n",
        )
        .with_command_writes(
            &makepkg,
            format!("{BUILD_DIRECTORY}/google-chrome-130.0-1-x86_64.pkg.tar.zst"),
            "",
        )
        .with_command_writes(
            &makepkg,
            format!("{BUILD_DIRECTORY}/google-chrome-debug-131.0-1-x86_64.pkg.tar.zst"),
            "",
        )
        .with_command_writes(
            &makepkg,
            format!("{BUILD_DIRECTORY}/google-chrome-131.0-1-x86_64.pkg.tar.zst"),
            "",
        )
}

fn changes(system: &FakeSystem) -> Vec<String> {
    system
        .journal()
        .into_iter()
        .filter(|entry| {
            !entry.starts_with("run pacman -Qq")
                && !entry.starts_with("connect ")
                && !is_state_entry(entry)
        })
        .collect()
}

#[test]
fn chrome_is_built_from_the_aur_by_the_build_user() {
    let system = with_chrome_build(first_boot());
    let config = config(Browser::Chrome, DeveloperTools::default());
    let summary = pass(&system, config, true);
    assert!(summary.is_complete(), "{summary:?}");

    assert_eq!(
        changes(&system),
        [
            "run id -u crownos-build".to_owned(),
            "run pacman -Syu --noconfirm".to_owned(),
            "run pacman -S --needed --asdeps --noconfirm -- fakeroot binutils debugedit file git"
                .to_owned(),
            format!("remove-all {BUILD_DIRECTORY}"),
            format!("mkdir {BUILD_DIRECTORY}"),
            format!("chown crownos-build {BUILD_DIRECTORY}"),
            format!(
                "run as crownos-build in {BUILD_DIRECTORY}: HOME={BUILD_DIRECTORY} git clone https://aur.archlinux.org/google-chrome.git {BUILD_DIRECTORY}"
            ),
            "run pacman -S --needed --asdeps --noconfirm -- gtk3 nss".to_owned(),
            format!(
                "run as crownos-build in {BUILD_DIRECTORY}: HOME={BUILD_DIRECTORY} makepkg --nodeps --nocheck --noconfirm --force"
            ),
            format!(
                "run pacman -U --noconfirm --needed -- {BUILD_DIRECTORY}/google-chrome-131.0-1-x86_64.pkg.tar.zst"
            ),
            "mkdir /etc/xdg".to_owned(),
            format!("write {MIMEAPPS}"),
        ]
    );

    system.clear_journal();
    assert!(pass(&system, config, true).is_complete());
    assert!(only_queries_ran(&system), "{:?}", system.journal());
}

#[test]
fn a_failed_pull_clones_the_package_again() {
    let pull = format!(
        "as crownos-build in {BUILD_DIRECTORY}: HOME={BUILD_DIRECTORY} git -C {BUILD_DIRECTORY} pull --ff-only"
    );
    let system = with_chrome_build(first_boot())
        .with_directory(format!("{BUILD_DIRECTORY}/.git"))
        .with_failure(&pull, "fatal: Not possible to fast-forward, aborting.");
    assert!(
        pass(
            &system,
            config(Browser::Chrome, DeveloperTools::default()),
            true
        )
        .is_complete()
    );

    let journal = system.journal();
    let pulled = journal
        .iter()
        .position(|entry| *entry == format!("run {pull}"))
        .expect("pulled");
    let removed = journal
        .iter()
        .position(|entry| *entry == format!("remove-all {BUILD_DIRECTORY}"))
        .expect("removed");
    assert!(pulled < removed);
    assert!(
        journal
            .iter()
            .skip(removed)
            .any(|entry| entry.contains("git clone"))
    );
}

#[test]
fn a_missing_build_user_fails_before_pacman() {
    let system = with_chrome_build(first_boot())
        .with_failure("id -u crownos-build", "id: 'crownos-build': no such user");
    let summary = pass(
        &system,
        config(Browser::Chrome, DeveloperTools::default()),
        true,
    );

    assert_eq!(
        summary.failed,
        [(
            SetupTask::Browser,
            "the system user `crownos-build` does not exist".to_owned()
        )]
    );
    assert!(
        !system
            .commands()
            .iter()
            .any(|command| command.starts_with("pacman -S"))
    );
}

const BRAVE_INSTALL: &str = "pacman -Syu --needed --noconfirm -- brave-bin";

fn failing_brave() -> FakeSystem {
    with_playwright_ready(first_boot())
        .with_failure(BRAVE_INSTALL, "error: target not found: brave-bin")
}

fn attempts(system: &FakeSystem, task: SetupTask) -> u32 {
    StateStore::new(system, SETUP_STATE_DIR)
        .load(task)
        .expect("loads")
        .map_or(0, |state| state.attempts)
}

#[test]
fn the_third_failure_gives_up_and_the_next_pass_skips_the_task() {
    let system = failing_brave();
    let brave = config(Browser::Brave, DeveloperTools::default());

    for attempt in 1..=2 {
        let summary = pass(&system, brave, true);
        assert_eq!(summary.failed.len(), 1, "attempt {attempt}");
        assert!(summary.given_up.is_empty());
        assert_eq!(attempts(&system, SetupTask::Browser), attempt);
    }
    let third = pass(&system, brave, true);
    assert!(third.failed.is_empty());
    assert_eq!(third.given_up, [SetupTask::Browser]);
    assert_eq!(attempts(&system, SetupTask::Browser), 3);

    system.clear_journal();
    let fourth = pass(&system, brave, true);
    assert_eq!(fourth.given_up, [SetupTask::Browser]);
    assert!(
        !system
            .journal()
            .iter()
            .any(|entry| entry.contains("brave") || entry.ends_with("browser.ron")),
        "{:?}",
        system.journal()
    );
    assert!(matches!(
        status(&system, SetupTask::Browser),
        Some(TaskStatus::Failed { .. })
    ));
}

#[test]
fn a_new_generation_resets_the_attempts() {
    let system = failing_brave();
    let brave = config(Browser::Brave, DeveloperTools::default());
    for _ in 0..3 {
        pass(&system, brave, true);
    }

    let context = Context::new(brave, &system);
    let summary = run_pass(
        &context,
        &StateStore::new(&system, SETUP_STATE_DIR),
        GENERATION + 1,
    )
    .expect("pass runs");
    assert_eq!(summary.failed.len(), 1);
    assert_eq!(attempts(&system, SetupTask::Browser), 1);
}

#[test]
fn a_done_or_pending_state_keeps_the_attempts() {
    let system = with_playwright_ready(first_boot());
    StateStore::new(&system, SETUP_STATE_DIR)
        .save(
            SetupTask::Browser,
            &TaskState {
                status: TaskStatus::Failed {
                    message: "offline".to_owned(),
                },
                generation: GENERATION,
                started_at_unix: None,
                finished_at_unix: None,
                attempts: 2,
            },
        )
        .expect("saves");
    pass(&system, SetupConfig::default(), true);
    assert_eq!(status(&system, SetupTask::Browser), Some(TaskStatus::Done));
    assert_eq!(attempts(&system, SetupTask::Browser), 2);
}

#[test]
fn a_failure_while_the_network_goes_down_is_not_an_attempt() {
    let system = failing_brave();
    let context = Context::new(config(Browser::Brave, DeveloperTools::default()), &system);
    system.set_connectivity(&[true, false]);
    let summary = run_pass(
        &context,
        &StateStore::new(&system, SETUP_STATE_DIR),
        GENERATION,
    )
    .expect("pass runs");

    assert!(summary.failed.is_empty(), "{summary:?}");
    assert_eq!(summary.waiting_for_network, [SetupTask::Browser]);
    assert_eq!(
        status(&system, SetupTask::Browser),
        Some(TaskStatus::Pending)
    );
    assert_eq!(attempts(&system, SetupTask::Browser), 0);
    assert!(system.commands().contains(&BRAVE_INSTALL.to_owned()));
}

#[test]
fn a_pass_without_network_tasks_does_not_probe_the_network() {
    let system = with_playwright_ready(first_boot());
    pass(&system, SetupConfig::default(), false);
    assert_eq!(system.network_probes(), 0);
}

#[test]
fn two_network_tasks_offline_probe_the_network_one_time() {
    let system = with_playwright_ready(first_boot());
    let summary = pass(&system, config(Browser::Brave, claude_code_only()), false);
    assert_eq!(
        summary.waiting_for_network,
        [SetupTask::Browser, SetupTask::DeveloperTools]
    );
    assert_eq!(system.network_probes(), 1);
}

#[test]
fn an_offline_failure_keeps_the_later_tasks_pending_without_a_probe() {
    let system = with_playwright_ready(first_boot()).with_failure(
        "pacman -Syu --needed --noconfirm -- brave-bin",
        "error: failed retrieving file",
    );
    let context = Context::new(config(Browser::Brave, claude_code_only()), &system);
    system.set_connectivity(&[true, false, true]);
    let summary = run_pass(
        &context,
        &StateStore::new(&system, SETUP_STATE_DIR),
        GENERATION,
    )
    .expect("pass runs");

    assert_eq!(
        summary.waiting_for_network,
        [SetupTask::Browser, SetupTask::DeveloperTools]
    );
    assert_eq!(system.network_probes(), 2);
    assert!(!system.is_installed("claude-code"));
}
