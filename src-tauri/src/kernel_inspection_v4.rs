//! Ephemeral UI inspection of an existing V4 kernel.

use crate::commands::AppState;
use omicsops_dto::{InspectKernelRequestV4, KernelInspectionV4};
use omicsops_protocol::{KernelInspectionStatusV4, KernelObjectMetadataV4};
use tauri::State;
use uuid::Uuid;

/// Returns an ephemeral snapshot; never stores metadata or starts execution resources.
#[tauri::command]
pub async fn agent_v4_inspect_kernel(
    state: State<'_, AppState>,
    request: InspectKernelRequestV4,
) -> Result<KernelInspectionV4, String> {
    crate::agent_v4::inspect_live_kernel_v4(&state, &request).await
}

pub(crate) fn project_inspection_response(
    request: &InspectKernelRequestV4,
    status: KernelInspectionStatusV4,
    session_id: Option<Uuid>,
    objects: Vec<KernelObjectMetadataV4>,
    truncated: bool,
) -> KernelInspectionV4 {
    let mut response = KernelInspectionV4 {
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        language: request.language,
        session_id,
        status,
        objects: vec![],
        truncated,
    };
    for mut object in objects {
        let lower = object.name.to_ascii_lowercase();
        // Mirror the worker name policy, and cover the existing host assignment keys.
        if object.name.starts_with('_')
            || [
                "password",
                "passwd",
                "secret",
                "token",
                "credential",
                "private_key",
                "api_key",
                "apikey",
                "access_key",
                "authorization",
                "passphrase",
            ]
            .iter()
            .any(|part| lower.contains(part))
        {
            response.truncated = true;
            continue;
        }
        object.name = crate::composer_references::public_text(&object.name);
        object.type_label = crate::composer_references::public_text(&object.type_label);
        object.summary = crate::composer_references::public_text(&object.summary);
        response.objects.push(object);
    }
    // Validate the complete UI envelope after scope projection and redaction.
    // A rejected snapshot is conservative and contains no worker text.
    if response.validate().is_err() {
        response.status = KernelInspectionStatusV4::Unavailable;
        response.objects.clear();
        response.truncated = false;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_protocol::*;
    use uuid::Uuid;
    fn request() -> InspectKernelRequestV4 {
        InspectKernelRequestV4 {
            project_id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            language: KernelLanguageV4::Python,
            expected_session_id: None,
        }
    }
    fn object(name: &str) -> KernelObjectMetadataV4 {
        KernelObjectMetadataV4 {
            name: name.into(),
            type_label: "int".into(),
            dimensions: None,
            element_count: Some(1),
            summary: "int".into(),
        }
    }
    #[test]
    fn inspection_host_redacts_ephemeral_metadata() {
        let mut ordinary = object("ordinary");
        ordinary.summary = "api_key=secret-sentinel".into();
        let response = project_inspection_response(
            &request(),
            KernelInspectionStatusV4::Ready,
            Some(Uuid::new_v4()),
            vec![
                object("api_key_secret_sentinel"),
                ordinary,
                object("_internal"),
            ],
            false,
        );
        let text = serde_json::to_string(&response).unwrap();
        assert!(!text.contains("secret-sentinel"));
        assert!(!text.contains("api_key_secret_sentinel"));
        assert!(!text.contains("_internal"));
        assert_eq!(response.objects.len(), 1);
        assert!(response.validate().is_ok());
    }
    #[test]
    fn inspection_host_rejects_near_limit_full_envelope() {
        let mut objects = vec![object("n"); 64];
        for index in 0..objects.len() {
            while objects[index].name.len() < 256
                && serde_json::to_vec(&KernelInspectionPayloadV4 {
                    objects: objects.clone(),
                    truncated: false,
                })
                .unwrap()
                .len()
                    < KERNEL_INSPECTION_MAX_RESPONSE_BYTES - 20
            {
                objects[index].name.push('n');
            }
        }
        let payload = KernelInspectionPayloadV4 {
            objects: objects.clone(),
            truncated: false,
        };
        assert!(validate_inspection_payload(&payload).is_ok());
        assert_eq!(
            serde_json::to_vec(&payload).unwrap().len(),
            KERNEL_INSPECTION_MAX_RESPONSE_BYTES - 20
        );
        let response = project_inspection_response(
            &request(),
            KernelInspectionStatusV4::Ready,
            Some(Uuid::new_v4()),
            objects,
            false,
        );
        assert_eq!(response.status, KernelInspectionStatusV4::Unavailable);
        assert!(response.objects.is_empty());
        assert!(response.validate().is_ok());
    }
}
