use omicsops_adapters::skills::{
    SkillEntry, inspect_skill_directory, install_skill_directory, validate_skill_entries,
};

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

#[test]
fn skill_import_is_content_addressed_and_copies_only_validated_files() {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("scripts")).unwrap();
    std::fs::write(
        source.path().join("SKILL.md"),
        "---\nname: scrna-qc\nversion: 1.2.0\ncapabilities:\n  - read_project_files\n---\n# scRNA QC\n",
    )
    .unwrap();
    std::fs::write(source.path().join("scripts/run.py"), "print('qc')\n").unwrap();
    let destination = tempfile::tempdir().unwrap();

    let first = install_skill_directory(source.path(), destination.path()).unwrap();
    let second = install_skill_directory(source.path(), destination.path()).unwrap();

    assert_eq!(first.sha256, second.sha256);
    assert_eq!(first.name, "scrna-qc");
    assert_eq!(first.version, "1.2.0");
    assert_eq!(first.capabilities, vec!["read_project_files"]);
    assert!(first.install_path.join("SKILL.md").is_file());
    assert!(first.install_path.join("scripts/run.py").is_file());
    assert!(
        first
            .install_path
            .starts_with(destination.path().canonicalize().unwrap())
    );
}

#[test]
fn skill_import_rejects_undeclared_capability_names() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("SKILL.md"),
        "---\nname: unsafe-skill\ncapabilities: [disable_approvals]\n---\n",
    )
    .unwrap();

    let error = inspect_skill_directory(source.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported skill capability"));
}
