//! Shared crate fid-x. It must not know which app is running.

use fid_registry_macros::fid_command;

// current_app() in a comment does not count, nor does FID_APP.
/* Block comments neither: AppId::new("a") /* nested current_app() */ */
const HELP: &str = "set FID_APP to choose an app; \"current_app()\"";
const RAW: &str = r#"current_app() in a "raw" string"#;

#[fid_command(id = "x.do_thing@1")]
pub fn do_thing() -> bool {
    let app = crate::host::current_app();
    let recurrent_apples = HELP.len() + RAW.len() + current_app_name().len();
    let quote = '"';
    app == "a" && recurrent_apples > 0 && quote != 'x'
}

fn current_app_name() -> &'static str {
    "AppId::from"
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_only_code_may_name_the_app() {
        let _ = std::env::var(FID_APP);
        let _ = super::AppId::from("a");
    }
}

pub fn configured() -> bool {
    let from_env = std::env::var("FID_APP").is_ok();
    let at_build = option_env!( "FID_APP" ).is_some();
    from_env && at_build && HELP.is_empty()
}

#[path = "../../outside/extra.rs"]
mod extra;
