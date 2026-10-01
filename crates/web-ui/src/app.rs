use gloo_net::http::{Request, Response};
use leptos::{ev::SubmitEvent, prelude::*};
use serde::{Serialize, de::DeserializeOwned};
use software_factory_api_types::{
    Architecture, CreateProjectRequest, ErrorResponse, Message, MessageRole, Project, ProjectTask,
    ProjectWorkspace, Requirement, RequirementKind, SendMessageRequest, SessionState, TaskStatus,
};
use wasm_bindgen_futures::spawn_local as spawn_browser_task;

use crate::interaction::{
    append_optimistic_user_message, display_project_message, requirement_view_key,
    should_submit_message,
};

const DEFAULT_API_BASE: &str = "http://127.0.0.1:3000";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Builder,
    Projects,
}

#[derive(Clone, Copy)]
enum RequirementAction {
    Pin(bool),
    Delete,
    Restore,
}

#[derive(Debug)]
struct UiError(String);

impl std::fmt::Display for UiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[component]
pub fn App() -> impl IntoView {
    let sessions = RwSignal::new(Vec::<SessionState>::new());
    let active_session = RwSignal::new(None::<SessionState>);
    let projects = RwSignal::new(Vec::<Project>::new());
    let screen = RwSignal::new(Screen::Builder);
    let selected_project = RwSignal::new(None::<Project>);
    let composer = RwSignal::new(String::new());
    let message_pending = RwSignal::new(false);
    let mutation_pending = RwSignal::new(false);
    let project_pending = RwSignal::new(false);
    let create_project_open = RwSignal::new(false);
    let project_name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let api_base = StoredValue::new(api_base_url());

    let send_message = Callback::new({
        let api_base = api_base.get_value();
        move |content: String| {
            if message_pending.get_untracked() || content.trim().is_empty() {
                return;
            }
            message_pending.set(true);
            error.set(None);
            let sessions = sessions;
            let active_session = active_session;
            let message_pending = message_pending;
            let error = error;
            let api_base = api_base.clone();
            spawn_browser_task(async move {
                let result = async {
                    let session = match active_session.get_untracked() {
                        Some(session) => session,
                        None => ApiClient::create_session(&api_base).await?,
                    };
                    let optimistic = append_optimistic_user_message(&session, &content);
                    replace_session(sessions, active_session, optimistic);
                    ApiClient::send_message(&api_base, &session.id, &content).await
                }
                .await;

                match result {
                    Ok(session) => replace_session(sessions, active_session, session),
                    Err(failure) => {
                        error.set(Some(failure.to_string()));
                        if let Some(session) = active_session.get_untracked()
                            && let Ok(latest) = ApiClient::get_session(&api_base, &session.id).await
                        {
                            replace_session(sessions, active_session, latest);
                        }
                    }
                }
                message_pending.set(false);
            });
        }
    });

    let requirement_action = Callback::new({
        let api_base = api_base.get_value();
        move |(requirement_id, action): (String, RequirementAction)| {
            mutate_requirement(
                api_base.clone(),
                active_session,
                sessions,
                mutation_pending,
                error,
                requirement_id,
                action,
            );
        }
    });

    Effect::new({
        let api_base = api_base.get_value();
        move |_| {
            let api_base = api_base.clone();
            spawn_browser_task(async move {
                match ApiClient::list_sessions(&api_base).await {
                    Ok(loaded) => {
                        if let Some(latest) = loaded.first().cloned() {
                            active_session.set(Some(latest));
                        }
                        sessions.set(loaded);
                    }
                    Err(failure) => error.set(Some(failure.to_string())),
                }
                match ApiClient::list_projects(&api_base).await {
                    Ok(loaded) => {
                        let running = loaded
                            .iter()
                            .filter(|project| is_project_active(&project.status))
                            .map(|project| project.id.clone())
                            .collect::<Vec<_>>();
                        projects.set(loaded);
                        for id in running {
                            poll_project(api_base.clone(), id, projects, project_pending, error);
                        }
                    }
                    Err(failure) => error.set(Some(failure.to_string())),
                }
            });
        }
    });

    view! {
        <div class="app-shell">
            <header class="topbar">
                <button
                    class="brand"
                    aria-label="Software Factory home"
                    on:click=move |_| screen.set(Screen::Builder)
                >
                    <span class="brand-mark">"sf"</span>
                    <span class="brand-name">"Software Factory"</span>
                </button>
                <nav class="top-nav" aria-label="Main navigation">
                    <button
                        class:nav-active=move || screen.get() == Screen::Builder
                        on:click=move |_| screen.set(Screen::Builder)
                    >"Builder"</button>
                    <button
                        class:nav-active=move || screen.get() == Screen::Projects
                        on:click=move |_| { selected_project.set(None); screen.set(Screen::Projects); }
                    >"Projects"</button>
                </nav>
                <div class="topbar-actions">
                    <span class="api-indicator"><i></i>"Local builder"</span>
                    <button
                        class="button button-primary button-small"
                        on:click=move |_| {
                            active_session.set(None);
                            composer.set(String::new());
                            screen.set(Screen::Builder);
                        }
                    >"New plan"</button>
                </div>
            </header>

            {move || {
                error.get().map(|message| {
                    view! {
                        <div class="error-banner" role="alert">
                            <span>{message}</span>
                            <button aria-label="Dismiss error" on:click=move |_| error.set(None)>
                                "×"
                            </button>
                        </div>
                    }
                })
            }}

            <Show
                when=move || screen.get() == Screen::Projects
                fallback=move || {
                    let api_base = StoredValue::new(api_base.get_value());
                    view! {
                        <Show
                            when=move || active_session.get().is_some()
                            fallback=move || {
                                view! {
                                    <Landing
                                        sessions=sessions
                                        composer=composer
                                        send_message=send_message
                                        pending=message_pending
                                        open_session=Callback::new(move |session: SessionState| {
                                            active_session.set(Some(session));
                                            screen.set(Screen::Builder);
                                        })
                                    />
                                }
                            }
                        >
                            <Workspace
                                sessions=sessions
                                active_session=active_session
                                composer=composer
                                send_message=send_message
                                message_pending=message_pending
                                mutation_pending=mutation_pending
                                requirement_action=requirement_action
                                create_project_open=create_project_open
                                project_name=project_name
                                project_pending=project_pending
                                projects=projects
                                api_base=api_base.get_value()
                            />
                        </Show>
                    }
                }
            >
                <Show when=move || selected_project.get().is_some() fallback=move || view! {
                    <ProjectLibrary
                        projects=projects project_pending=project_pending api_base=api_base.get_value() error=error
                        open_project=Callback::new(move |project| selected_project.set(Some(project)))
                    />
                }>
                    {move || selected_project.get().map(|project| view! {
                        <ProjectWorkspaceView project=project api_base=api_base.get_value()
                            go_back=Callback::new(move |()| selected_project.set(None)) />
                    })}
                </Show>
            </Show>

            <Show when=move || create_project_open.get()>
                <CreateProjectDialog
                    active_session=active_session
                    project_name=project_name
                    project_pending=project_pending
                    create_project_open=create_project_open
                    projects=projects
                    error=error
                    api_base=api_base.get_value()
                    screen=screen
                />
            </Show>
        </div>
    }
}

#[component]
fn Landing(
    sessions: RwSignal<Vec<SessionState>>,
    composer: RwSignal<String>,
    send_message: Callback<String>,
    pending: RwSignal<bool>,
    open_session: Callback<SessionState>,
) -> impl IntoView {
    view! {
        <main class="landing-page">
            <div class="landing-glow landing-glow-left"></div>
            <div class="landing-glow landing-glow-right"></div>
            <section class="hero-content">
                <div class="eyebrow"><span class="sparkle">"✦"</span>"YOUR LOCAL AI SOFTWARE BUILDER"</div>
                <h1>"What will you "<span>"build today?"</span></h1>
                <p class="hero-subtitle">
                    "Shape the architecture, agree on requirements, then let your agents build it."
                </p>
                <Composer
                    value=composer
                    on_send=send_message
                    pending=pending
                    expanded=true
                    placeholder="Describe the software you want to create..."
                />
                <div class="starter-prompts">
                    <span class="starter-label">"GET STARTED"</span>
                    <button on:click=move |_| composer.set("A simple website for my new business".to_owned())>
                        <span>"✦"</span>"A business website"
                    </button>
                    <button on:click=move |_| composer.set("A small app to organize personal projects".to_owned())>
                        <span>"◈"</span>"A project app"
                    </button>
                    <button on:click=move |_| composer.set("An API service for managing customer data".to_owned())>
                        <span>"⌘"</span>"An API service"
                    </button>
                </div>

                <Show when=move || !sessions.get().is_empty()>
                    <section class="recent-sessions">
                        <div class="section-kicker">"PICK UP WHERE YOU LEFT OFF"</div>
                        <div class="recent-session-list">
                            <For
                                each=move || { sessions.get().into_iter().take(3).collect::<Vec<_>>() }
                                key=|session| session.id.clone()
                                children=move |session| {
                                    let title = session_title(&session);
                                    let selected = session.clone();
                                    view! {
                                        <button class="recent-session" on:click=move |_| open_session.run(selected.clone())>
                                            <span class="recent-icon">"◌"</span>
                                            <span>{title}</span>
                                            <span class="recent-arrow">"↗"</span>
                                        </button>
                                    }
                                }
                            />
                        </div>
                    </section>
                </Show>
            </section>
            <div class="landing-footer">"Your ideas stay on your machine."</div>
        </main>
    }
}

#[component]
fn Workspace(
    sessions: RwSignal<Vec<SessionState>>,
    active_session: RwSignal<Option<SessionState>>,
    composer: RwSignal<String>,
    send_message: Callback<String>,
    message_pending: RwSignal<bool>,
    mutation_pending: RwSignal<bool>,
    requirement_action: Callback<(String, RequirementAction)>,
    create_project_open: RwSignal<bool>,
    project_name: RwSignal<String>,
    project_pending: RwSignal<bool>,
    projects: RwSignal<Vec<Project>>,
    api_base: String,
) -> impl IntoView {
    view! {
        <main class="workspace-page">
            <aside class="session-rail">
                <div class="rail-heading">
                    <span>"YOUR PLANS"</span>
                    <button
                        class="icon-button"
                        title="New plan"
                        on:click=move |_| {
                            active_session.set(None);
                            composer.set(String::new());
                        }
                    >"+"</button>
                </div>
                <div class="session-list">
                    <For
                        each=move || sessions.get()
                        key=|session| session.id.clone()
                        children=move |session| {
                            let title = session_title(&session);
                            let selected = session.clone();
                            let session_id = session.id.clone();
                            view! {
                                <button
                                    class="session-entry"
                                    class:session-selected=move || active_session.with(|active| {
                                        active.as_ref().is_some_and(|active| active.id == session_id)
                                    })
                                    on:click=move |_| active_session.set(Some(selected.clone()))
                                >
                                    <span class="session-entry-icon">"◌"</span>
                                    <span class="session-entry-title">{title}</span>
                                </button>
                            }
                        }
                    />
                </div>
                <div class="rail-bottom">
                    <div class="local-note"><span class="local-dot"></span>"Running locally"</div>
                    <span class="api-address">{api_base.clone()}</span>
                </div>
            </aside>

            <section class="chat-workspace">
                <div class="workspace-titlebar">
                    <div>
                        <div class="section-kicker">"PLANNING SESSION"</div>
                        <h2>{move || active_session.with(|active| active.as_ref().map(session_title).unwrap_or_else(|| "New project plan".to_owned()))}</h2>
                    </div>
                    <span class="agent-chip"><i></i>"Planning agent"</span>
                </div>
                <div class="conversation-scroll">
                    <Show
                        when=move || active_session.with(|active| active.as_ref().is_some_and(|session| !session.messages.is_empty()))
                        fallback=move || {
                            view! {
                                <div class="chat-empty-state">
                                    <div class="empty-orb">"✦"</div>
                                    <h3>"Let's define your project."</h3>
                                    <p>"Tell the planning agent what you have in mind. Your architecture and requirements will take shape as you talk."</p>
                                </div>
                            }
                        }
                    >
                        <div class="message-list">
                            <For
                                each=move || active_session.get().map(|session| session.messages).unwrap_or_default()
                                key=|message| message.id.clone()
                                children=move |message| view! { <ChatMessage message=message /> }
                            />
                            <Show when=move || message_pending.get()>
                                <div class="assistant-message pending-message">
                                    <span class="assistant-avatar">"✦"</span>
                                    <div class="typing-indicator"><i></i><i></i><i></i><span>"Thinking through your plan..."</span></div>
                                </div>
                            </Show>
                        </div>
                    </Show>
                </div>
                <Composer
                    value=composer
                    on_send=send_message
                    pending=message_pending
                    expanded=false
                    placeholder="Reply to the planning agent..."
                />
                <p class="privacy-note">"The planning conversation and generated projects are stored locally."</p>
            </section>

            <PlanningPanel
                active_session=active_session
                mutation_pending=mutation_pending
                requirement_action=requirement_action
                create_project_open=create_project_open
                project_name=project_name
                project_pending=project_pending
                projects=projects
            />
        </main>
    }
}

#[component]
fn ChatMessage(message: Message) -> impl IntoView {
    let is_assistant = message.role == MessageRole::Assistant;
    let content = message.content;
    view! {
        <article class="message-row" class:user-row=!is_assistant class:assistant-row=is_assistant>
            {if is_assistant {
                view! { <div class="assistant-avatar">"✦"</div> }.into_any()
            } else {
                view! { <div class="user-avatar">"You"</div> }.into_any()
            }}
            <div class="message-body">
                <div class="message-speaker">{if is_assistant { "Planning agent" } else { "You" }}</div>
                <div class="message-content">{content}</div>
            </div>
        </article>
    }
}

#[component]
fn PlanningPanel(
    active_session: RwSignal<Option<SessionState>>,
    mutation_pending: RwSignal<bool>,
    requirement_action: Callback<(String, RequirementAction)>,
    create_project_open: RwSignal<bool>,
    project_name: RwSignal<String>,
    project_pending: RwSignal<bool>,
    projects: RwSignal<Vec<Project>>,
) -> impl IntoView {
    view! {
        <aside class="plan-panel">
            <div class="plan-panel-heading">
                <div>
                    <div class="section-kicker">"WORKING AGREEMENT"</div>
                    <h2>"Project plan"</h2>
                </div>
                <span class="plan-status"><i></i>"Live"</span>
            </div>

            <section class="plan-section architecture-section">
                <div class="plan-section-title">
                    <span class="section-icon">"⌘"</span>
                    <h3>"Architecture & stack"</h3>
                </div>
                <Show
                    when=move || active_session.with(|active| active.as_ref().is_some_and(|session| session.architecture.is_some()))
                    fallback=move || view! {
                        <div class="plan-empty">
                            "The architecture and technology choices will appear here as you agree on them with the agent."
                        </div>
                    }
                >
                    {move || active_session.get().and_then(|session| session.architecture).map(|architecture| view! {
                        <ArchitectureView architecture=architecture />
                    })}
                </Show>
            </section>

            <section class="plan-section requirements-section">
                <div class="plan-section-title requirement-heading">
                    <div>
                        <span class="section-icon">"☷"</span>
                        <h3>"Requirements"</h3>
                    </div>
                    <span class="count-pill">{move || active_session.with(|active| active.as_ref().map(|session| session.requirements.len()).unwrap_or(0))}</span>
                </div>
                <Show
                    when=move || active_session.with(|active| active.as_ref().is_some_and(|session| !session.requirements.is_empty()))
                    fallback=move || view! { <div class="plan-empty">"Agreed requirements will be collected here."</div> }
                >
                    <RequirementGroup active_session=active_session kind=RequirementKind::Functional label="Functional requirements" pending=mutation_pending action=requirement_action />
                    <RequirementGroup active_session=active_session kind=RequirementKind::NonFunctional label="Non-functional requirements" pending=mutation_pending action=requirement_action />
                    <RequirementGroup active_session=active_session kind=RequirementKind::Unclassified label="Awaiting classification" pending=mutation_pending action=requirement_action />
                </Show>

                <section class="open-questions">
                    <h3>"Open questions"</h3>
                    {move || active_session.get().map(|session| session.questions.into_iter().map(|question| view! { <p>{question}</p> }).collect_view())}
                </section>

                <details class="deleted-requirements">
                    <summary>
                        <span>"Deleted requirements"</span>
                        <span class="count-pill muted-count">{move || active_session.with(|active| active.as_ref().map(|session| session.deleted_requirements.len()).unwrap_or(0))}</span>
                    </summary>
                    <div class="deleted-list">
                        <For
                            each=move || active_session.get().map(|session| session.deleted_requirements).unwrap_or_default()
                            key=|requirement| requirement.id.clone()
                            children=move |requirement| {
                                let requirement_id = requirement.id.clone();
                                let action = requirement_action;
                                let pending = mutation_pending;
                                view! {
                                    <div class="deleted-item">
                                        <span>{format!("{} · {:?}", requirement.text, requirement.kind)}
                                            <ul>{requirement.acceptance_criteria.into_iter().map(|criterion| view! { <li>{criterion}</li> }).collect_view()}</ul>
                                        </span>
                                        <button
                                            class="text-button"
                                            disabled=move || pending.get()
                                            on:click=move |_| action.run((requirement_id.clone(), RequirementAction::Restore))
                                        >"Restore"</button>
                                    </div>
                                }
                            }
                        />
                        <Show when=move || active_session.with(|active| active.as_ref().is_some_and(|session| session.deleted_requirements.is_empty()))>
                            <p class="no-deleted">"Deleted requirements stay here until you explicitly restore them."</p>
                        </Show>
                    </div>
                </details>
            </section>

            <div class="plan-panel-footer">
                <div class="project-readiness">
                    <span class="readiness-dot" class:ready=move || active_session.with(is_ready_to_build)></span>
                    <span>{move || if active_session.with(is_ready_to_build) { "Plan ready to build" } else { "Keep shaping your plan" }}</span>
                </div>
                <button
                    class="button button-primary build-button"
                    disabled=move || project_pending.get() || !active_session.with(is_ready_to_build)
                    on:click=move |_| {
                        project_name.set(String::new());
                        create_project_open.set(true);
                    }
                >
                    <span>"Create project"</span><span class="button-arrow">"↗"</span>
                </button>
                <Show when=move || !projects.get().is_empty()>
                    <div class="recent-project-note">
                        {move || format!("{} project(s) created", projects.get().len())}
                    </div>
                </Show>
            </div>
        </aside>
    }
}

#[component]
fn ArchitectureView(architecture: Architecture) -> impl IntoView {
    let overview = architecture.overview;
    let stack = architecture.stack;
    let decisions = architecture.decisions;
    let has_stack = !stack.is_empty();
    let has_decisions = !decisions.is_empty();
    let stack_items = StoredValue::new(stack);
    let decision_items = StoredValue::new(decisions);
    view! {
        <div class="architecture-content">
            <p class="architecture-overview">{overview}</p>
            <Show when=move || has_stack>
                <div class="stack-list">
                    <For
                        each=move || stack_items.get_value()
                        key=|choice| format!("{}:{}", choice.category, choice.technology)
                        children=move |choice| view! {
                            <div class="stack-choice">
                                <span class="stack-category">{choice.category}</span>
                                <strong>{choice.technology}</strong>
                                <p>{choice.rationale}</p>
                            </div>
                        }
                    />
                </div>
            </Show>
            <Show when=move || has_decisions>
                <div class="decision-list">
                    <div class="subsection-label">"KEY DECISIONS"</div>
                    <For
                        each=move || decision_items.get_value()
                        key=|decision| decision.topic.clone()
                        children=move |decision| view! {
                            <div class="decision-item">
                                <strong>{decision.topic}</strong>
                                <p>{decision.decision}</p>
                                <small>{decision.rationale}</small>
                            </div>
                        }
                    />
                </div>
            </Show>
        </div>
    }
}

#[component]
fn RequirementCard(
    requirement: Requirement,
    pending: RwSignal<bool>,
    on_action: Callback<RequirementAction>,
) -> impl IntoView {
    let pinned = requirement.pinned;
    view! {
        <article class="requirement-card">
            <span class="requirement-marker" class:pinned-marker=pinned></span>
            <div class="requirement-content">
                <p>{requirement.text}</p>
                <ul class="acceptance-criteria">{requirement.acceptance_criteria.into_iter().map(|criterion| view! { <li>{criterion}</li> }).collect_view()}</ul>
            </div>
            <div class="requirement-actions">
                <button
                    class="text-button pin-button"
                    title=if pinned { "Unpin requirement" } else { "Pin requirement" }
                    disabled=move || pending.get()
                    on:click=move |_| on_action.run(RequirementAction::Pin(!pinned))
                >
                    {if pinned { "◆ Pinned" } else { "◇ Pin" }}
                </button>
                <button
                    class="icon-button delete-button"
                    title="Delete requirement"
                    disabled=move || pending.get()
                    on:click=move |_| on_action.run(RequirementAction::Delete)
                >"×"</button>
            </div>
        </article>
    }
}

#[component]
fn Composer(
    value: RwSignal<String>,
    on_send: Callback<String>,
    pending: RwSignal<bool>,
    expanded: bool,
    placeholder: &'static str,
    #[prop(default = "Planning agent")] agent_label: &'static str,
) -> impl IntoView {
    let on_submit = move |event: SubmitEvent| {
        event.prevent_default();
        send_composer_value(value, pending, on_send);
    };
    let on_keydown = move |event: leptos::ev::KeyboardEvent| {
        if should_submit_message(&event.key(), event.shift_key()) {
            event.prevent_default();
            send_composer_value(value, pending, on_send);
        }
    };

    view! {
        <form class="composer" class:composer-expanded=expanded on:submit=on_submit>
            <textarea
                rows=if expanded { 3 } else { 2 }
                prop:value=move || value.get()
                placeholder=placeholder
                aria-label=format!("Message the {agent_label}")
                disabled=move || pending.get()
                on:input=move |event| value.set(event_target_value(&event))
                on:keydown=on_keydown
            ></textarea>
            <div class="composer-toolbar">
                <div class="composer-context">
                    <span class="agent-light"></span>
                    <span>{agent_label}</span>
                    <span class="composer-divider"></span>
                    <span class="composer-local">"Local project"</span>
                </div>
                <div class="composer-submit-group">
                    <span class="composer-hint">{if expanded { "Clarify ideas together" } else { "Your plan updates as you talk" }}</span>
                    <button
                        class="send-button"
                        type="submit"
                        aria-label="Send message"
                        disabled=move || pending.get() || value.with(|text| text.trim().is_empty())
                    >
                        {move || if pending.get() { "…" } else { "↗" }}
                    </button>
                </div>
            </div>
        </form>
    }
}

#[component]
fn ProjectLibrary(
    projects: RwSignal<Vec<Project>>,
    project_pending: RwSignal<bool>,
    api_base: String,
    error: RwSignal<Option<String>>,
    open_project: Callback<Project>,
) -> impl IntoView {
    let refresh = move |_| {
        let api_base = api_base.clone();
        let projects = projects;
        let project_pending = project_pending;
        let error = error;
        spawn_browser_task(async move {
            project_pending.set(true);
            match ApiClient::list_projects(&api_base).await {
                Ok(loaded) => projects.set(loaded),
                Err(failure) => error.set(Some(failure.to_string())),
            }
            project_pending.set(false);
        });
    };

    view! {
        <main class="project-library">
            <div class="library-heading library-heading-projects">
                <div class="library-title">
                    <div class="section-kicker">"WORKSPACE / PROJECTS"</div>
                    <h1>"Projects"</h1>
                    <p>"Local workspaces, architecture and implementation progress."</p>
                </div>
                <div class="library-tools">
                    <span class="library-count">{move || {
                        let count = projects.get().len();
                        format!("{count} PROJECT{}", if count == 1 { "" } else { "S" })
                    }}</span>
                    <button class="button button-secondary" on:click=refresh disabled=move || project_pending.get()>
                        <span aria-hidden="true">"↻"</span>"Refresh"
                    </button>
                </div>
            </div>
            <Show
                when=move || !projects.get().is_empty()
                fallback=move || view! {
                    <div class="library-empty">
                        <div class="empty-orb" aria-hidden="true">"P"</div>
                        <h2>"No projects yet"</h2>
                        <p>"Create a project from an agreed plan to start a workspace with its own orchestrator and task board."</p>
                    </div>
                }
            >
                <div class="project-grid">
                    <For
                        each=move || projects.get()
                        key=|project| format!("{}:{}:{:?}", project.id, project.status, project.error)
                        children=move |project| view! { <ProjectCard project=project open_project=open_project /> }
                    />
                </div>
            </Show>
        </main>
    }
}

#[component]
fn ProjectCard(project: Project, open_project: Callback<Project>) -> impl IntoView {
    let selected = StoredValue::new(project.clone());
    let is_running = project.status == "running";
    let is_queued = project.status == "queued";
    let is_failed = project.status == "failed";
    let is_completed = project.status == "completed";
    let status_label = project_status_label(&project.status).to_owned();
    let status_detail = match project.status.as_str() {
        "queued" => "Waiting for an implementation worker",
        "running" => "Initial implementation is in progress",
        "failed" => "Review the error before continuing",
        _ => "Workspace ready for new changes",
    };
    let monogram = project
        .name
        .chars()
        .next()
        .unwrap_or('P')
        .to_uppercase()
        .to_string();
    let requirement_count = project.snapshot.requirements.len();
    let stack_count = project.snapshot.architecture.stack.len();
    let project_error = project.error;
    view! {
        <article class="project-card">
            <div class="project-card-top">
                <span class="project-card-icon" aria-hidden="true">{monogram}</span>
                <span class="project-status" class:status-running=is_running class:status-queued=is_queued class:status-failed=is_failed class:status-completed=is_completed>
                    <i></i>{status_label}
                </span>
            </div>
            <h2>{project.name}</h2>
            <p class="project-card-summary">{status_detail}</p>
            <div class="project-path-block"><span>"PROJECT DIRECTORY"</span><code class="project-path">{project.path}</code></div>
            <div class="project-card-meta">
                <span><strong>{requirement_count}</strong> " requirements"</span>
                <span><strong>{stack_count}</strong> " technologies"</span>
            </div>
            {project_error.map(|message| view! { <p class="project-error">{message}</p> })}
            <button class="button button-secondary project-open-button" disabled=!is_completed on:click=move |_| open_project.run(selected.get_value())>
                "Open workspace"<span aria-hidden="true">"→"</span>
            </button>
        </article>
    }
}

fn project_status_label(status: &str) -> &'static str {
    match status {
        "queued" => "Queued",
        "running" => "Building",
        "failed" => "Needs attention",
        _ => "Ready",
    }
}

fn requirement_kind_label(kind: &RequirementKind) -> &'static str {
    match kind {
        RequirementKind::Functional => "FUNCTIONAL",
        RequirementKind::NonFunctional => "NON-FUNCTIONAL",
        RequirementKind::Unclassified => "UNCLASSIFIED",
    }
}

#[component]
fn RequirementGroup(
    active_session: RwSignal<Option<SessionState>>,
    kind: RequirementKind,
    label: &'static str,
    pending: RwSignal<bool>,
    action: Callback<(String, RequirementAction)>,
) -> impl IntoView {
    let kind = StoredValue::new(kind);
    view! {
        <div class="requirement-group">
            <h4>{label}</h4>
            <div class="requirement-list">
                <For each=move || active_session.get().map(|s| s.requirements.into_iter().filter(|r| r.kind == kind.get_value()).collect::<Vec<_>>()).unwrap_or_default()
                    key=requirement_view_key children=move |requirement| {
                        let id = requirement.id.clone();
                        view! { <RequirementCard requirement=requirement pending=pending on_action=Callback::new(move |next| action.run((id.clone(), next))) /> }
                    } />
            </div>
        </div>
    }
}

#[component]
fn ProjectWorkspaceView(
    project: Project,
    api_base: String,
    go_back: Callback<()>,
) -> impl IntoView {
    let workspace = RwSignal::new(None::<ProjectWorkspace>);
    let composer = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let project = StoredValue::new(project);
    let base = StoredValue::new(api_base);
    let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cleanup_alive = alive.clone();
    on_cleanup(move || cleanup_alive.store(false, std::sync::atomic::Ordering::Relaxed));

    let send = Callback::new({
        let alive = alive.clone();
        move |content: String| {
            if pending.get_untracked() || content.trim().is_empty() {
                return;
            }
            pending.set(true);
            error.set(None);
            workspace.update(|state| {
                if let Some(state) = state {
                    state.messages.push(Message {
                        id: format!("pending-{}", state.revision),
                        role: MessageRole::User,
                        content: content.clone(),
                        created_at: 0,
                    });
                }
            });
            let api_base = base.get_value();
            let id = project.get_value().id;
            let alive = alive.clone();
            spawn_browser_task(async move {
                let result: Result<ProjectWorkspace, UiError> = post_json(
                    format!("{api_base}/api/projects/{id}/messages"),
                    &SendMessageRequest { content },
                )
                .await;
                if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match result {
                    Ok(state) => workspace.set(Some(state)),
                    Err(failure) => {
                        error.set(Some(failure.to_string()));
                        if let Ok(state) = get_json::<ProjectWorkspace>(format!(
                            "{api_base}/api/projects/{id}/workspace"
                        ))
                        .await
                            && alive.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            workspace.set(Some(state));
                        }
                    }
                }
                if alive.load(std::sync::atomic::Ordering::Relaxed) {
                    pending.set(false);
                }
            });
        }
    });
    let poll_alive = alive.clone();
    let api_base = base.get_value();
    let project_id = project.get_value().id;
    spawn_browser_task(async move {
        loop {
            if !poll_alive.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if !pending.get_untracked() {
                let result = get_json::<ProjectWorkspace>(format!(
                    "{api_base}/api/projects/{project_id}/workspace"
                ))
                .await;
                if !poll_alive.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if !pending.get_untracked() {
                    match result {
                        Ok(state) => workspace.set(Some(state)),
                        Err(failure) => error.set(Some(failure.to_string())),
                    }
                }
            }
            gloo_timers::future::TimeoutFuture::new(1_000).await;
        }
    });

    view! {
        <main class="project-workspace">
            <div class="project-breadcrumbs">
                <button class="text-button" on:click=move |_| go_back.run(())><span aria-hidden="true">"←"</span>"All projects"</button>
                <span aria-hidden="true">"/"</span><span>{project.get_value().name}</span>
            </div>
            <header class="project-heading">
                <div class="project-heading-main">
                    <div class="section-kicker">"PROJECT WORKSPACE"</div>
                    <h1>{project.get_value().name}</h1>
                    <p>{project.get_value().snapshot.architecture.overview.clone()}</p>
                </div>
                <div class="project-heading-status">
                    <span class="project-status status-completed"><i></i>"Ready"</span>
                    <span class="repository-indicator"><span aria-hidden="true">"⌘"</span>" Local repository · main"</span>
                </div>
            </header>
            <div class="project-context-row">
                <span class="context-label">"DIRECTORY"</span><code>{project.get_value().path}</code>
                <span class="context-separator"></span>
                <span>{project.get_value().snapshot.requirements.len()} " agreed requirements"</span>
                <details class="project-architecture">
                    <summary>"Architecture & requirements"</summary>
                    <div class="project-architecture-content">
                        <ArchitectureView architecture=project.get_value().snapshot.architecture />
                        <h3>"Agreed requirements"</h3>
                        {project.get_value().snapshot.requirements.into_iter().map(|r| view! {
                            <article class="project-requirement">
                                <span class="requirement-type">{requirement_kind_label(&r.kind)}</span>
                                <p>{r.text}</p>
                                <ul>{r.acceptance_criteria.into_iter().map(|c| view! { <li>{c}</li> }).collect_view()}</ul>
                            </article>
                        }).collect_view()}
                    </div>
                </details>
            </div>
            {move || error.get().map(|message| view! { <div class="error-banner" role="alert">{message}</div> })}
            <div class="project-collaboration">
                <section class="orchestrator-chat">
                    <div class="section-heading">
                        <div><div class="section-kicker">"DISCUSS & DEFINE"</div><h2>"Project orchestrator"</h2></div>
                        <span class="orchestrator-presence"><i></i>"Project agent"</span>
                    </div>
                    <div class="project-messages" aria-live="polite">
                        <Show when=move || workspace.with(|w| w.as_ref().is_some_and(|w| !w.messages.is_empty())) fallback=move || view! {
                            <div class="chat-empty-state"><span class="chat-empty-mark">"↗"</span><strong>"Start with the change you want to make"</strong><p>"The orchestrator will clarify scope, then prepare a plan for your review."</p></div>
                        }>
                        {move || workspace.get().map(|w| w.messages.into_iter().map(|m| view! {
                            <article class="project-message" class:user-message=m.role == MessageRole::User>
                                <div class="message-author"><span class="message-avatar">{if m.role == MessageRole::User { "Y" } else { "O" }}</span><strong>{if m.role == MessageRole::User { "You" } else { "Orchestrator" }}</strong></div>
                                <p>{display_project_message(&m)}</p>
                            </article>
                        }).collect_view())}
                        </Show>
                        <Show when=move || pending.get()><p>"Orchestrator is responding…"</p></Show>
                    </div>
                    <Composer value=composer on_send=send pending=pending expanded=false placeholder="Describe a change or new feature…" agent_label="Project orchestrator" />
                </section>
                <section class="project-plans">
                    <div class="section-heading">
                        <div><div class="section-kicker">"NEXT STEP"</div><h2>"Implementation plan"</h2></div>
                        {move || workspace.with(|w| view! { <span class="plan-count">{w.as_ref().map(|w| w.plans.len()).unwrap_or_default()}</span> })}
                    </div>
                    <p class="panel-description">"Review the proposed scope and approve it to queue work for implementation."</p>
                    <div class="project-plan-list">
                        {move || workspace.get().map(|w| {
                            let tasks = w.tasks;
                            w.plans.into_iter().rev().map(|plan| {
                                let count = tasks.iter().filter(|task| task.plan_id == plan.id).count();
                                view! { <PlanCard plan=plan task_count=count send=send pending=pending /> }
                            }).collect_view()
                        })}
                        <Show when=move || workspace.with(|w| w.as_ref().is_some_and(|w| w.plans.is_empty()))>
                            <div class="plan-empty-state"><span>"Plans appear here after you discuss a change."</span></div>
                        </Show>
                    </div>
                </section>
            </div>
            <section class="task-board" aria-label="Read-only task kanban">
                <div class="task-board-heading">
                    <div><div class="section-kicker">"DELIVERY"</div><h2>"Task board"</h2><p>"Execution progress updates automatically. Task scope and status are managed by agents."</p></div>
                    <div class="board-heading-meta">
                        {move || workspace.with(|w| {
                            let count = w.as_ref().map(|w| w.tasks.len()).unwrap_or_default();
                            view! { <span class="board-count">{format!("{count} task{}", if count == 1 { "" } else { "s" })}</span> }
                        })}
                        <span class="board-scroll-hint"><span class="board-scroll-desktop">"Scroll to view all stages"</span><span class="board-scroll-mobile">"Swipe to view all stages"</span><span aria-hidden="true">"→"</span></span>
                    </div>
                </div>
                <Show when=move || workspace.with(|w| w.as_ref().is_some_and(|w| !w.tasks.is_empty())) fallback=move || view! {
                    <div class="board-empty-state"><span class="board-empty-icon">"01"</span><div><strong>"No tasks to track yet"</strong><p>"Discuss a change with the orchestrator. Approved work will appear in the board."</p></div></div>
                }>
                    <div class="kanban-viewport" tabindex="0" aria-label="Scroll horizontally to view all task statuses">
                        <div class="kanban-grid">
                            {[TaskStatus::PendingApproval, TaskStatus::Queued, TaskStatus::Implementing, TaskStatus::Verifying, TaskStatus::Integrating, TaskStatus::Completed, TaskStatus::Blocked, TaskStatus::Failed].into_iter().map(|status| {
                                let label = task_status_label(&status);
                                let status_class = task_status_class(&status).to_owned();
                                let indicator_class = format!("lane-indicator {status_class}");
                                let status_value = StoredValue::new(status);
                                view! { <section class="kanban-column">
                                    <header class="kanban-column-heading"><span class=indicator_class></span><h3>{label}</h3>
                                        <span class="lane-count">{move || workspace.with(|w| w.as_ref().map(|w| w.tasks.iter().filter(|t| t.status == status_value.get_value()).count()).unwrap_or(0))}</span>
                                    </header>
                                    <div class="kanban-cards">
                                        <For each=move || workspace.get().map(|w| w.tasks.into_iter().filter(|t| t.status == status_value.get_value()).collect::<Vec<_>>()).unwrap_or_default()
                                            key=|task| task.id.clone()
                                            children=move |task| view! { <TaskCard task=task /> } />
                                        <Show when=move || workspace.with(|w| w.as_ref().is_some_and(|w| !w.tasks.iter().any(|t| t.status == status_value.get_value())))>
                                            <p class="kanban-empty">"No tasks"</p>
                                        </Show>
                                    </div>
                                </section> }
                            }).collect_view()}
                        </div>
                    </div>
                </Show>
            </section>
        </main>
    }
}

#[component]
fn PlanCard(
    plan: software_factory_api_types::ProjectPlan,
    task_count: usize,
    send: Callback<String>,
    pending: RwSignal<bool>,
) -> impl IntoView {
    let command = StoredValue::new(format!("confirm {}", plan.id));
    let approval_available = !plan.approved && !plan.superseded;
    let status = if plan.superseded {
        "Replaced"
    } else if plan.approved {
        "Approved"
    } else {
        "Needs review"
    };
    view! {
        <article class="project-plan" class:plan-superseded=plan.superseded>
            <div class="plan-status-line"><span class="plan-state-marker" class:plan-state-approved=plan.approved class:plan-state-replaced=plan.superseded></span><span>{status}</span></div>
            <h3>{plan.summary}</h3>
            <div class="plan-metadata"><span>{task_count} " tasks"</span><span>"Acceptance criteria included"</span></div>
            <Show when=move || approval_available>
                <div class="approval-note"><strong>"Approval starts implementation"</strong><span>"Agents will create branches and update the task board as they work."</span></div>
                <button class="button button-primary plan-approve-button" disabled=move || pending.get() on:click=move |_| send.run(command.get_value())>
                    {move || if pending.get() { "Sending approval…" } else { "Approve plan and queue tasks" }}
                </button>
            </Show>
        </article>
    }
}

fn task_status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::PendingApproval => "Awaiting approval",
        TaskStatus::Queued => "Queued",
        TaskStatus::Implementing => "Implementing",
        TaskStatus::Verifying => "Verifying",
        TaskStatus::Integrating => "Integrating",
        TaskStatus::Completed => "Completed",
        TaskStatus::Blocked => "Blocked",
        TaskStatus::Failed => "Failed",
    }
}

fn task_status_class(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::PendingApproval => "lane-pending",
        TaskStatus::Queued => "lane-queued",
        TaskStatus::Implementing => "lane-active",
        TaskStatus::Verifying => "lane-verifying",
        TaskStatus::Integrating => "lane-integrating",
        TaskStatus::Completed => "lane-completed",
        TaskStatus::Blocked => "lane-blocked",
        TaskStatus::Failed => "lane-failed",
    }
}

#[component]
fn TaskCard(task: ProjectTask) -> impl IntoView {
    let criteria_count = task.spec.acceptance_criteria.len();
    let activity_count = task.activity.len();
    view! {
        <details class="task-card">
            <summary><span class="task-card-title">{task.spec.title}</span><span class="task-expand-label">"Details"</span></summary>
            <p class="task-card-description">{task.spec.description}</p>
            <div class="task-card-meta"><span>{criteria_count} " acceptance checks"</span><span>{activity_count} " updates"</span></div>
            <div class="task-card-details">
                <div class="task-detail-group"><h4>"Acceptance criteria"</h4><ul>{task.spec.acceptance_criteria.into_iter().map(|c| view! { <li>{c}</li> }).collect_view()}</ul></div>
                <div class="task-detail-group"><h4>"Verification"</h4>{task.spec.verification_commands.into_iter().map(|c| view! { <code>{c}</code> }).collect_view()}</div>
                {(!task.spec.dependencies.is_empty()).then(|| view! { <div class="task-detail-group"><h4>"Dependencies"</h4><p>{format!("Depends on {} earlier task(s) in this plan", task.spec.dependencies.len())}</p></div> })}
                {task.error.map(|value| view! { <div class="task-error"><strong>"Needs attention"</strong><p>{value}</p></div> })}
                <details class="task-activity"><summary>{format!("Execution details · {} updates", activity_count)}</summary>
                    <ul>{task.activity.into_iter().map(|a| view! { <li>{a}</li> }).collect_view()}</ul>
                    <dl class="task-git-details">
                        {task.branch.map(|value| view! { <div><dt>"Branch"</dt><dd><code>{value}</code></dd></div> })}
                        {task.base_commit.map(|value| view! { <div><dt>"Started from"</dt><dd><code>{value}</code></dd></div> })}
                        {task.commit.map(|value| view! { <div><dt>"Commit"</dt><dd><code>{value}</code></dd></div> })}
                        {task.worktree.map(|value| view! { <div><dt>"Worktree"</dt><dd><code>{value}</code></dd></div> })}
                    </dl>
                </details>
            </div>
        </details>
    }
}

#[component]
fn CreateProjectDialog(
    active_session: RwSignal<Option<SessionState>>,
    project_name: RwSignal<String>,
    project_pending: RwSignal<bool>,
    create_project_open: RwSignal<bool>,
    projects: RwSignal<Vec<Project>>,
    error: RwSignal<Option<String>>,
    api_base: String,
    screen: RwSignal<Screen>,
) -> impl IntoView {
    let submit = move |event: SubmitEvent| {
        event.prevent_default();
        if project_pending.get_untracked() {
            return;
        }
        let Some(session) = active_session.get_untracked() else {
            return;
        };
        let name = project_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            return;
        }
        project_pending.set(true);
        error.set(None);
        let session_id = session.id;
        let api_base = api_base.clone();
        let projects = projects;
        let project_pending = project_pending;
        let create_project_open = create_project_open;
        let error = error;
        let screen = screen;
        spawn_browser_task(async move {
            match ApiClient::create_project(&api_base, &session_id, &name).await {
                Ok(project) => {
                    replace_project(projects, project.clone());
                    create_project_open.set(false);
                    screen.set(Screen::Projects);
                    if is_project_active(&project.status) {
                        poll_project(api_base, project.id, projects, project_pending, error);
                    }
                }
                Err(failure) => error.set(Some(failure.to_string())),
            }
            project_pending.set(false);
        });
    };

    view! {
        <div class="modal-backdrop" role="presentation">
            <section class="create-dialog" role="dialog" aria-modal="true" aria-labelledby="create-dialog-title">
                <button
                    class="icon-button dialog-close"
                    aria-label="Close dialog"
                    on:click=move |_| create_project_open.set(false)
                    disabled=move || project_pending.get()
                >"×"</button>
                <div class="dialog-icon">"✦"</div>
                <div class="section-kicker">"IMPLEMENTATION AGENT"</div>
                <h2 id="create-dialog-title">"Ready to build?"</h2>
                <p>"Your agreed architecture and active requirements will be sent to an implementation agent."</p>
                <form on:submit=submit>
                    <label for="project-name">"Project name"</label>
                    <input
                        id="project-name"
                        type="text"
                        maxlength="100"
                        autocomplete="off"
                        placeholder="e.g. customer-dashboard"
                        prop:value=move || project_name.get()
                        on:input=move |event| project_name.set(event_target_value(&event))
                        disabled=move || project_pending.get()
                    />
                    <div class="dialog-path">
                        <span>"Destination"</span>
                        <code>{move || format!("$HOME/sf/projects/{}", project_name.get())}</code>
                    </div>
                    <button
                        class="button button-primary dialog-submit"
                        type="submit"
                        disabled=move || project_pending.get() || project_name.with(|name| name.trim().is_empty())
                    >
                        {move || if project_pending.get() { "Starting agent…" } else { "Create project" }}
                        <span>"↗"</span>
                    </button>
                </form>
            </section>
        </div>
    }
}

fn mutate_requirement(
    api_base: String,
    active_session: RwSignal<Option<SessionState>>,
    sessions: RwSignal<Vec<SessionState>>,
    pending: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    requirement_id: String,
    action: RequirementAction,
) {
    if pending.get_untracked() {
        return;
    }
    let Some(session) = active_session.get_untracked() else {
        return;
    };
    pending.set(true);
    error.set(None);
    let session_id = session.id;
    spawn_browser_task(async move {
        let result = match action {
            RequirementAction::Pin(pinned) => {
                ApiClient::set_pinned(&api_base, &session_id, &requirement_id, pinned).await
            }
            RequirementAction::Delete => {
                ApiClient::delete_requirement(&api_base, &session_id, &requirement_id).await
            }
            RequirementAction::Restore => {
                ApiClient::restore_requirement(&api_base, &session_id, &requirement_id).await
            }
        };
        match result {
            Ok(session) => replace_session(sessions, active_session, session),
            Err(failure) => error.set(Some(failure.to_string())),
        }
        pending.set(false);
    });
}

fn replace_session(
    sessions: RwSignal<Vec<SessionState>>,
    active_session: RwSignal<Option<SessionState>>,
    updated: SessionState,
) {
    active_session.set(Some(updated.clone()));
    sessions.update(|all| {
        if let Some(existing) = all.iter_mut().find(|session| session.id == updated.id) {
            *existing = updated;
        } else {
            all.insert(0, updated);
        }
    });
}

fn send_composer_value(
    value: RwSignal<String>,
    pending: RwSignal<bool>,
    on_send: Callback<String>,
) {
    if pending.get_untracked() {
        return;
    }
    let content = value.get_untracked().trim().to_owned();
    if !content.is_empty() {
        value.set(String::new());
        on_send.run(content);
    }
}

fn replace_project(projects: RwSignal<Vec<Project>>, updated: Project) {
    projects.update(|all| {
        if let Some(existing) = all.iter_mut().find(|project| project.id == updated.id) {
            *existing = updated;
        } else {
            all.insert(0, updated);
        }
    });
}

fn poll_project(
    api_base: String,
    project_id: String,
    projects: RwSignal<Vec<Project>>,
    pending: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) {
    spawn_browser_task(async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
            match ApiClient::get_project(&api_base, &project_id).await {
                Ok(project) => {
                    let finished = !is_project_active(&project.status);
                    replace_project(projects, project);
                    if finished {
                        pending.set(false);
                        break;
                    }
                }
                Err(failure) => {
                    error.set(Some(failure.to_string()));
                    pending.set(false);
                    break;
                }
            }
        }
    });
}

fn is_project_active(status: &str) -> bool {
    matches!(status, "queued" | "running")
}

fn is_ready_to_build(session: &Option<SessionState>) -> bool {
    session
        .as_ref()
        .is_some_and(|session| session.architecture.is_some() && !session.requirements.is_empty())
}

fn session_title(session: &SessionState) -> String {
    session
        .messages
        .iter()
        .find(|message| message.role == MessageRole::User)
        .map(|message| {
            let title = message.content.lines().next().unwrap_or("New project plan");
            let mut chars = title.chars();
            let preview = chars.by_ref().take(38).collect::<String>();
            if chars.next().is_some() {
                format!("{preview}…")
            } else {
                preview
            }
        })
        .unwrap_or_else(|| "New project plan".to_owned())
}

fn api_base_url() -> String {
    option_env!("SF_API_BASE_URL")
        .unwrap_or(DEFAULT_API_BASE)
        .trim_end_matches('/')
        .to_owned()
}

struct ApiClient;

impl ApiClient {
    async fn list_sessions(base: &str) -> Result<Vec<SessionState>, UiError> {
        get_json(format!("{base}/api/sessions")).await
    }

    async fn create_session(base: &str) -> Result<SessionState, UiError> {
        let response = Request::post(&format!("{base}/api/sessions"))
            .send()
            .await
            .map_err(network_error)?;
        decode_json(response).await
    }

    async fn get_session(base: &str, session_id: &str) -> Result<SessionState, UiError> {
        get_json(format!("{base}/api/sessions/{session_id}")).await
    }

    async fn send_message(
        base: &str,
        session_id: &str,
        content: &str,
    ) -> Result<SessionState, UiError> {
        post_json(
            format!("{base}/api/sessions/{session_id}/messages"),
            &SendMessageRequest {
                content: content.to_owned(),
            },
        )
        .await
    }

    async fn set_pinned(
        base: &str,
        session_id: &str,
        requirement_id: &str,
        pinned: bool,
    ) -> Result<SessionState, UiError> {
        let url = format!("{base}/api/sessions/{session_id}/requirements/{requirement_id}/pin");
        let response = if pinned {
            Request::put(&url).send().await
        } else {
            Request::delete(&url).send().await
        }
        .map_err(network_error)?;
        decode_json(response).await
    }

    async fn delete_requirement(
        base: &str,
        session_id: &str,
        requirement_id: &str,
    ) -> Result<SessionState, UiError> {
        let response = Request::delete(&format!(
            "{base}/api/sessions/{session_id}/requirements/{requirement_id}"
        ))
        .send()
        .await
        .map_err(network_error)?;
        decode_json(response).await
    }

    async fn restore_requirement(
        base: &str,
        session_id: &str,
        requirement_id: &str,
    ) -> Result<SessionState, UiError> {
        let response = Request::post(&format!(
            "{base}/api/sessions/{session_id}/deleted-requirements/{requirement_id}/restore"
        ))
        .send()
        .await
        .map_err(network_error)?;
        decode_json(response).await
    }

    async fn list_projects(base: &str) -> Result<Vec<Project>, UiError> {
        get_json(format!("{base}/api/projects")).await
    }

    async fn get_project(base: &str, project_id: &str) -> Result<Project, UiError> {
        get_json(format!("{base}/api/projects/{project_id}")).await
    }

    async fn create_project(base: &str, session_id: &str, name: &str) -> Result<Project, UiError> {
        post_json(
            format!("{base}/api/sessions/{session_id}/projects"),
            &CreateProjectRequest {
                name: name.to_owned(),
            },
        )
        .await
    }
}

async fn get_json<T: DeserializeOwned>(url: String) -> Result<T, UiError> {
    let response = Request::get(&url).send().await.map_err(network_error)?;
    decode_json(response).await
}

async fn post_json<B: Serialize, T: DeserializeOwned>(url: String, body: &B) -> Result<T, UiError> {
    let response = Request::post(&url)
        .json(body)
        .map_err(network_error)?
        .send()
        .await
        .map_err(network_error)?;
    decode_json(response).await
}

async fn decode_json<T: DeserializeOwned>(response: Response) -> Result<T, UiError> {
    if !(200..300).contains(&response.status()) {
        let status = response.status();
        let message = response
            .json::<ErrorResponse>()
            .await
            .map(|payload| payload.error)
            .unwrap_or_else(|_| format!("Request failed with HTTP status {status}"));
        return Err(UiError(message));
    }
    response.json::<T>().await.map_err(network_error)
}

fn network_error(error: impl std::fmt::Display) -> UiError {
    UiError(format!("Could not reach the local builder API: {error}"))
}
