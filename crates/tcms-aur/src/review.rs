use tcms_core::{Error, Result};
pub fn arguments(helper: &str, extra: &[String], target: Option<&str>) -> Result<Vec<String>> {
    let name = std::path::Path::new(helper)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !matches!(name, "paru" | "yay") {
        return Err(Error::Message(tcms_core::t("aur.review_helper")));
    }
    for arg in extra {
        if [
            "--noconfirm",
            "--skipreview",
            "--noreview",
            "--nodiffmenu",
            "--noeditmenu",
            "--answerdiff",
            "--answeredit",
        ]
        .iter()
        .any(|blocked| arg == blocked || arg.starts_with(&format!("{blocked}=")))
        {
            return Err(Error::Message(tcms_core::t("aur.review_unsafe_args")));
        }
    }
    let mut args = vec![if target.is_some() { "-S" } else { "-Sua" }.into()];
    args.extend_from_slice(extra);
    args.push("--confirm".into());
    if name == "paru" {
        args.push("--review".into());
    } else {
        args.extend(
            [
                "--diffmenu",
                "--editmenu",
                "--answerdiff",
                "All",
                "--answeredit",
                "All",
            ]
            .into_iter()
            .map(str::to_string),
        );
    }
    if let Some(target) = target {
        args.extend(["--needed".into(), "--".into(), target.into()]);
    }
    Ok(args)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installs_and_upgrades_request_interactive_review() {
        for helper in ["/usr/bin/paru", "/usr/bin/yay"] {
            for target in [Some("example-bin"), None] {
                let args = arguments(helper, &[], target).unwrap();
                assert!(args.contains(&"--confirm".into()));
                assert!(!args.contains(&"--noconfirm".into()));
                assert!(!args.contains(&"--skipreview".into()));
                assert!(args.contains(
                    &if helper.ends_with("paru") {
                        "--review"
                    } else {
                        "--editmenu"
                    }
                    .into()
                ));
            }
        }
    }
    #[test]
    fn disabling_review_is_rejected() {
        for option in [
            "--skipreview",
            "--noconfirm",
            "--answeredit=None",
            "--answerdiff",
            "--nodiffmenu",
        ] {
            assert!(arguments("paru", &[option.into()], Some("example")).is_err());
        }
        assert!(arguments("custom-helper", &[], Some("example")).is_err());
    }
}
