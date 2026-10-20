#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Exercise the installed native door against real source and the scan built
//! with it; no model call, application build or real Claude session is needed.
#[path = "support/binary.rs"]
mod binary;

use std::{fs,process::Command};
use serde_json::{Value,json};

#[test]
fn native_installation_prepares_the_index_and_an_update_preserves_personal_choices() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path();
    assert!(Command::new("git").args(["init","-q"]).current_dir(root).status().unwrap().success());
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/sample.py"),"def quartz():\n    return 42\n").unwrap();
    fs::write(root.join("mustard.json"),json!({"version":"0.0.0","installationChoice":"preserve","ai":{"fallback":false,"vectors":true}}).to_string()).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("mustard.json"),fs::Permissions::from_mode(0o600)).unwrap();
    }
    let upsert=|| {
        let output=binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run","upsert"]).current_dir(root)
            .env("CLAUDE_CONFIG_DIR",root.join("isolated-host"))
            .env("CLAUDE_PROJECT_DIR",root).env("MUSTARD_RT_DELEGATED","1")
            .env_remove("CLAUDE_PLUGIN_ROOT").env_remove("TYPESAFE_API_KEY").env_remove("JEV_API_KEY")
            .output().unwrap();
        assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let first=upsert();
    assert_eq!(first["installation"]["scan"]["ok"],true,"{first}");
    assert!(root.join(".claude/grain.db").is_file());
    assert_eq!(first["installation"]["host_ready"],false);
    assert_eq!(first["installation"]["complete"],false);
    assert_eq!(first["installation"]["provider_verified"],false);
    assert!(!root.join(".claude/judgements/attempts.ndjson").exists());
    let second=upsert();
    assert_eq!(second["installation"]["scan"]["read"],0,"unchanged sources must not be read again");
    let config:Value=serde_json::from_str(&fs::read_to_string(root.join("mustard.json")).unwrap()).unwrap();
    assert_eq!(config["version"],mustard_core::harness_version());
    assert_eq!(config["installationChoice"],"preserve");
    assert_eq!(config["ai"]["fallback"],false);
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(root.join("mustard.json")).unwrap().permissions().mode() & 0o777,0o600);
    }
    let receipt:Value=serde_json::from_str(&fs::read_to_string(root.join(".claude/mustard/installation.json")).unwrap()).unwrap();
    assert_eq!(receipt,second["installation"]);
    assert!(!root.join(".claude/mustard/pages/project.html").exists());
}
