use gloo_net::http::{Request, Response};
use leptos::{ev::SubmitEvent, prelude::*};
use serde::{Serialize, de::DeserializeOwned};
use software_factory_api_types::{
    Architecture, CreateProjectRequest, ErrorResponse, Message, MessageRole, Project, Requirement,
    SendMessageRequest, SessionState,
};
use wasm_bindgen_futures::spawn_local as spawn_browser_task;

use crate::interaction::{
    append_optimistic_user_message, requirement_view_key, should_submit_message,
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
    let composer = RwSignal::new(String::new());
    let message_pending = RwSignal::new(false);
    let mutation_pending = RwSignal::new(false);
    let project_pending = RwSignal::new(false);
    let create_project_open = RwSignal::new(false);
    let project_name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let api_base = StoredValue::new(api_base_url());

    let send_message = Callback::new({
        let sessions = sessions;
        let active_session = active_session;
        let message_pending = message_pending;
        let error = error;
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
                        None => {
                            let session = ApiClient::create_session(&api_base).await?;
                            session
                        }
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
        let sessions = sessions;
        let active_session = active_session;
        let mutation_pending = mutation_pending;
        let error = error;
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
        let sessions = sessions;
        let active_session = active_session;
        let projects = projects;
        let error = error;
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
                        on:click=move |_| screen.set(Screen::Projects)
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
                <ProjectLibrary
                    projects=projects
                    project_pending=project_pending
                    api_base=api_base.get_value()
                    error=error
                />
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
                    <div class="requirement-list">
                        <For
                            each=move || active_session.get().map(|session| session.requirements).unwrap_or_default()
                            key=requirement_view_key
                            children=move |requirement| {
                                let requirement_id = requirement.id.clone();
                                let action = requirement_action;
                                let pending = mutation_pending;
                                view! {
                                    <RequirementCard
                                        requirement=requirement
                                        pending=pending
                                        on_action=Callback::new(move |next| action.run((requirement_id.clone(), next)))
                                    />
                                }
                            }
                        />
                    </div>
                </Show>

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
                                        <span>{requirement.text}</span>
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
            <p>{requirement.text}</p>
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
                aria-label="Message the planning agent"
                disabled=move || pending.get()
                on:input=move |event| value.set(event_target_value(&event))
                on:keydown=on_keydown
            ></textarea>
            <div class="composer-toolbar">
                <div class="composer-context">
                    <span class="agent-light"></span>
                    <span>"Planning agent"</span>
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
            <div class="library-heading">
                <div>
                    <div class="section-kicker">"BUILT WITH SOFTWARE FACTORY"</div>
                    <h1>"Your projects"</h1>
                    <p>"Projects created by your implementation agents, all in one place."</p>
                </div>
                <button class="button button-secondary" on:click=refresh disabled=move || project_pending.get()>
                    <span>"↻"</span>"Refresh"
                </button>
            </div>
            <Show
                when=move || !projects.get().is_empty()
                fallback=move || view! {
                    <div class="library-empty">
                        <div class="empty-orb">"⌘"</div>
                        <h2>"Your first project starts with a plan."</h2>
                        <p>"Chat with the planning agent, agree on the architecture and requirements, then create a project."</p>
                    </div>
                }
            >
                <div class="project-grid">
                    <For
                        each=move || projects.get()
                        key=|project| project.id.clone()
                        children=move |project| view! { <ProjectCard project=project /> }
                    />
                </div>
            </Show>
        </main>
    }
}

#[component]
fn ProjectCard(project: Project) -> impl IntoView {
    let is_running = project.status == "running";
    let is_failed = project.status == "failed";
    let is_completed = project.status == "completed";
    let status_label = project.status.clone();
    let project_error = project.error;
    view! {
        <article class="project-card">
            <div class="project-card-top">
                <span class="project-card-icon">"⌘"</span>
                <span class="project-status" class:status-running=is_running class:status-failed=is_failed class:status-completed=is_completed>
                    <i></i>{status_label}
                </span>
            </div>
            <h2>{project.name}</h2>
            <p class="project-path">{project.path}</p>
            <div class="project-card-meta">
                <span>{format!("{} requirement(s)", project.snapshot.requirements.len())}</span>
                <span>{project.snapshot.architecture.stack.len()} " stack choices"</span>
            </div>
            {project_error.map(|message| view! { <p class="project-error">{message}</p> })}
        </article>
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
