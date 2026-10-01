use software_factory_api_types::{Message, MessageRole, Requirement, SessionState};

pub(crate) fn should_submit_message(key: &str, shift_pressed: bool) -> bool {
    key == "Enter" && !shift_pressed
}

pub(crate) fn append_optimistic_user_message(
    session: &SessionState,
    content: &str,
) -> SessionState {
    let mut optimistic = session.clone();
    optimistic.messages.push(Message {
        id: format!("pending-{}", session.revision + 1),
        role: MessageRole::User,
        content: content.to_owned(),
        created_at: session.updated_at,
    });
    optimistic
}

pub(crate) fn requirement_view_key(requirement: &Requirement) -> String {
    format!(
        "{}:{}:{}:{:?}:{:?}",
        requirement.id,
        requirement.pinned,
        requirement.text,
        requirement.kind,
        requirement.acceptance_criteria
    )
}

pub(crate) fn display_project_message(message: &Message) -> String {
    if message.role == MessageRole::User && message.content.starts_with("confirm ") {
        return "Approved the plan for implementation.".into();
    }
    if message.role == MessageRole::Assistant
        && message.content.starts_with("Plan ")
        && message.content.contains(" confirmed.")
    {
        return "Plan approved. Its tasks are eligible for execution; dependencies will be respected.".into();
    }
    if message.role == MessageRole::Assistant
        && let Some((summary, _)) = message
            .content
            .split_once("\n\nReview the plan and send `confirm ")
    {
        return format!(
            "{summary}\n\nReview the proposed tasks and approve them in the implementation plan to authorize this version."
        );
    }
    message.content.clone()
}

#[cfg(test)]
mod tests {
    use software_factory_api_types::{Architecture, MessageRole, Requirement, SessionState};

    use super::{
        append_optimistic_user_message, display_project_message, requirement_view_key,
        should_submit_message,
    };

    #[test]
    fn enter_submits_and_shift_enter_keeps_a_newline() {
        assert!(should_submit_message("Enter", false));
        assert!(!should_submit_message("Enter", true));
        assert!(!should_submit_message("Escape", false));
    }

    #[test]
    fn user_message_is_added_to_the_local_session_immediately() {
        let session = SessionState {
            id: "session-1".to_owned(),
            revision: 4,
            architecture: Some(Architecture {
                overview: "A local app".to_owned(),
                stack: vec![],
                decisions: vec![],
            }),
            messages: vec![],
            requirements: vec![],
            deleted_requirements: vec![],
            created_at: 1,
            updated_at: 9,
            questions: vec![],
        };

        let optimistic = append_optimistic_user_message(&session, "Build the app");

        assert!(session.messages.is_empty());
        assert_eq!(optimistic.messages.len(), 1);
        assert_eq!(optimistic.messages[0].role, MessageRole::User);
        assert_eq!(optimistic.messages[0].content, "Build the app");
        assert_eq!(optimistic.messages[0].id, "pending-5");
    }

    #[test]
    fn pin_state_change_replaces_the_keyed_requirement_row() {
        let unpinned = Requirement {
            id: "req-1".to_owned(),
            text: "Persist user data".to_owned(),
            pinned: false,
            kind: software_factory_api_types::RequirementKind::Functional,
            acceptance_criteria: vec!["Data survives restart".into()],
        };
        let mut pinned = unpinned.clone();
        pinned.pinned = true;

        assert_ne!(
            requirement_view_key(&unpinned),
            requirement_view_key(&pinned)
        );
    }

    #[test]
    fn confirmation_messages_hide_internal_plan_identifiers() {
        use software_factory_api_types::Message;

        fn message(role: MessageRole, content: &str) -> Message {
            Message {
                id: "message-id".into(),
                role,
                content: content.into(),
                created_at: 0,
            }
        }

        let proposal = message(
            MessageRole::Assistant,
            "Review this plan\n\nReview the plan and send `confirm plan-123` to authorize this exact version.",
        );
        let approved = message(MessageRole::User, "confirm plan-123");
        let response = message(
            MessageRole::Assistant,
            "Plan plan-123 confirmed. Its tasks are eligible for execution; dependencies will be respected.",
        );

        for visible in [
            display_project_message(&proposal),
            display_project_message(&approved),
            display_project_message(&response),
        ] {
            assert!(!visible.contains("plan-123"));
        }
        assert!(
            display_project_message(&proposal).contains("approve them in the implementation plan")
        );
        assert_eq!(
            display_project_message(&approved),
            "Approved the plan for implementation."
        );
    }
}
