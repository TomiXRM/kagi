use super::command_execution_allowed;
use std::ffi::OsString;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const SIGNALS: &[&str] = &[
    "KAGI_OPEN_REPO",
    "KAGI_MENU_DUMP",
    "KAGI_SELECT_FIRST",
    "KAGI_NO_SINGLE_INSTANCE",
];

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[test]
fn no_single_instance_allows_command_execution() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _restore = EnvRestore(
        SIGNALS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect(),
    );
    for key in SIGNALS {
        std::env::remove_var(key);
    }
    std::env::set_var("KAGI_NO_SINGLE_INSTANCE", "1");

    assert!(command_execution_allowed());
}
