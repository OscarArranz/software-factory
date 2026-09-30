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
        "{}:{}:{}",
        requirement.id, requirement.pinned, requirement.text
    )
}

#[cfg(test)]
mod tests {
    use software_factory_api_types::{Architecture, MessageRole, Requirement, SessionState};

    use super::{append_optimistic_user_message, requirement_view_key, should_submit_message};

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
        };
        let mut pinned = unpinned.clone();
        pinned.pinned = true;

        assert_ne!(
            requirement_view_key(&unpinned),
            requirement_view_key(&pinned)
        );
    }
}
