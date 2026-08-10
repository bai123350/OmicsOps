use omicsops_adapters::skills::{SkillEntry, validate_skill_entries};

#[test]
fn skill_import_rejects_path_traversal_and_absolute_entries() {
    for path in [
        "../escape",
        "assets/../../escape",
        "/absolute/file",
        "C:\\escape",
    ] {
        assert!(
            validate_skill_entries(&[SkillEntry::file(path)]).is_err(),
            "accepted {path}"
        );
    }
}

#[test]
fn skill_import_rejects_symlinks_that_escape_package_root() {
    assert!(
        validate_skill_entries(&[
            SkillEntry::file("SKILL.md"),
            SkillEntry::symlink("assets/latest", "../../outside"),
        ])
        .is_err()
    );
    assert!(
        validate_skill_entries(&[
            SkillEntry::file("SKILL.md"),
            SkillEntry::symlink("assets/latest", "figures/current.png"),
        ])
        .is_ok()
    );
}

#[test]
fn skill_package_requires_a_root_instruction_file() {
    assert!(validate_skill_entries(&[SkillEntry::file("scripts/run.py")]).is_err());
    assert!(
        validate_skill_entries(&[
            SkillEntry::file("SKILL.md"),
            SkillEntry::file("scripts/run.py")
        ])
        .is_ok()
    );
}
