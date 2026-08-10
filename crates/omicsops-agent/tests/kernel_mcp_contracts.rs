use omicsops_agent::{
    ApprovalDecision, ApprovalGate, KernelLanguage, KernelSession, KernelState,
    McpServerDeclaration, McpTransport,
};
use uuid::Uuid;

#[test]
fn first_release_mcp_accepts_only_stdio_and_requires_launch_approval() {
    let declaration = McpServerDeclaration::new(
        "literature",
        McpTransport::Stdio {
            command: "paper-mcp".into(),
            args: vec!["--jsonl".into()],
        },
    );
    assert!(declaration.validate().is_ok());
    assert_eq!(
        declaration.approval(&ApprovalGate),
        ApprovalDecision::Required
    );
    assert!(
        McpServerDeclaration::new(
            "web",
            McpTransport::Http {
                url: "http://127.0.0.1:3000".into()
            }
        )
        .validate()
        .is_err()
    );
}

#[test]
fn interrupted_kernels_rebuild_only_from_saved_code_cells() {
    let mut session = KernelSession::new(Uuid::new_v4(), Uuid::new_v4(), KernelLanguage::Python);
    session.start().unwrap();
    session.save_cell("import scanpy as sc");
    session.note_ephemeral_cell("adata.obs.head()");
    session.interrupt();
    assert_eq!(session.state, KernelState::Interrupted);
    assert_eq!(session.rebuild_cells(), vec!["import scanpy as sc"]);
}

#[test]
fn exploratory_code_must_be_saved_before_formal_promotion() {
    let mut session = KernelSession::new(Uuid::new_v4(), Uuid::new_v4(), KernelLanguage::R);
    session.start().unwrap();
    session.note_ephemeral_cell("obj <- NormalizeData(obj)");
    assert!(session.promote_cell(0, "normalize", 1).is_err());
    session.save_cell("obj <- NormalizeData(obj)");
    let proposal = session.promote_cell(0, "normalize", 1).unwrap();
    assert_eq!(proposal.version, 1);
    assert_eq!(proposal.code, "obj <- NormalizeData(obj)");
    assert!(!proposal.code_sha256.is_empty());
}
