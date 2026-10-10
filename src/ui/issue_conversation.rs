//! Session-owned accepted Issue projection and its single variable-height list.
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use gpui::{
    px, App, AppContext as _, Context, ElementId, Entity, ListOffset, ListState, SharedString,
    WeakEntity, Window,
};
use gpui_component::{
    text::{TextSelectionGroupRetirement, TextViewState, TextViewStyle},
    Root,
};
use kagi_domain::github::{Issue, IssueState};

use super::{
    i18n::Msg,
    timeline_row::{BodyMarkdownFormat, PreparedBody},
    KagiApp,
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum PostKey {
    Body,
    Comment(SharedString),
}

pub(super) struct Post {
    pub key: PostKey,
    pub author: String,
    pub created_at: String,
    pub comment_index: Option<usize>,
    pub prepared: PreparedBody,
    pub text: Entity<TextViewState>,
    source_revision: u64,
}

#[derive(Default)]
pub(super) struct ConversationScope {
    generation: Cell<u64>,
    active: Cell<bool>,
    retirement: RefCell<Option<TextSelectionGroupRetirement>>,
}

impl ConversationScope {
    pub fn retire(&self) {
        if let Some(retirement) = self.retirement.borrow_mut().take() {
            retirement.retire();
        }
        self.generation.set(self.generation.get().wrapping_add(1));
        self.active.set(false);
    }
}

pub(super) struct ConversationActivation(pub Rc<ConversationScope>);

impl Drop for ConversationActivation {
    fn drop(&mut self) {
        self.0.retire();
    }
}

impl Drop for IssueConversation {
    fn drop(&mut self) {
        self.scope.retire();
    }
}

pub(super) struct IssueConversation {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub posts: Vec<Post>,
    pub list: ListState,
    pub group: ElementId,
    members: Vec<(ElementId, Entity<TextViewState>)>,
    root: Option<WeakEntity<Root>>,
    pub scope: Rc<ConversationScope>,
    geometry_style: Option<TextViewStyle>,
}

impl IssueConversation {
    pub fn generation(&self) -> u64 {
        self.scope.generation.get()
    }
    pub fn active(&self) -> bool {
        self.scope.active.get()
            && self
                .scope
                .retirement
                .borrow()
                .as_ref()
                .is_none_or(TextSelectionGroupRetirement::is_active)
    }

    pub fn new(number: u64, cx: &mut Context<Self>) -> Self {
        Self {
            number,
            title: String::new(),
            state: IssueState::Unknown,
            posts: Vec::new(),
            list: ListState::new(2, gpui::ListAlignment::Top, px(400.))
                .with_uniform_item_height(px(120.)),
            group: ("issue-conversation", cx.entity_id()).into(),
            members: Vec::new(),
            root: None,
            scope: Rc::new(ConversationScope::default()),
            geometry_style: None,
        }
    }

    pub fn retire(&mut self, cx: &mut App) {
        let was_active = self.scope.active.get();
        self.scope.retire();
        if !was_active {
            return;
        }
        if let Some(root) = self.root.as_ref().and_then(WeakEntity::upgrade) {
            root.update(cx, |root, cx| root.set_text_selection_group(None, cx));
        }
    }

    pub fn activate(&mut self, cx: &mut App) {
        if self.active() {
            return;
        }
        self.scope.active.set(true);
        self.scope
            .generation
            .set(self.scope.generation.get().wrapping_add(1));
        self.publish_group(cx);
    }

    fn publish_group(&self, cx: &mut App) {
        if self.scope.active.get() {
            if let Some(root) = self.root.as_ref().and_then(WeakEntity::upgrade) {
                let retirement = root.update(cx, |root, cx| {
                    root.set_text_selection_group(Some((self.group.clone(), &self.members)), cx)
                });
                *self.scope.retirement.borrow_mut() = retirement;
            }
        }
    }

    pub fn bind_window(
        entity: &Entity<Self>,
        app: &Entity<KagiApp>,
        session: Option<crate::app::SessionId>,
        consumer: u64,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(root) = window.root::<Root>().flatten() else {
            return;
        };
        let weak = root.downgrade();
        let style = super::timeline_row::markdown_style(15., cx);
        let needs_install = entity.update(cx, |owner, _| {
            if owner
                .geometry_style
                .as_ref()
                .is_none_or(|old| old != &style || old.is_dark != style.is_dark)
            {
                owner.list.remeasure_items(0..owner.list.item_count());
                owner.geometry_style = Some(style);
            }
            let root_changed = owner.root.as_ref().is_none_or(|old| old != &weak);
            owner.root = Some(weak);
            root_changed
                || !owner.active()
                || owner.posts.first().is_some_and(|post| {
                    matches!(post.prepared.format, BodyMarkdownFormat::Placeholder { text, .. }
                    if text != Msg::IssueNoDescription.t())
                })
        });
        if !needs_install {
            return;
        }
        let weak = entity.downgrade();
        let weak_app = app.downgrade();
        let generation = entity.read(cx).generation();
        // Root is leased during Kagi's render; install after those leases return.
        window.defer(cx, move |_, cx| {
            let Some(app) = weak_app.upgrade() else {
                return;
            };
            let current = app.read(cx);
            if current.active_session() != session
                || current.ui().issue_conversation_gen != consumer
                || current.home_in_front()
                || current.has_active_modal()
                || current.workspace_mode() != super::workspace_mode::WorkspaceMode::Issues
                || current.menu_overlay.is_some()
                || current.conflict_body_visible()
                || current.has_modal_or_visible_plan(cx)
            {
                return;
            }
            let _ = weak.update(cx, |owner, cx| {
                if owner.generation() != generation {
                    return;
                }
                owner.sync_placeholder(cx);
                if owner.active() {
                    owner.publish_group(cx);
                } else {
                    owner.activate(cx);
                }
            });
            app.update(cx, |_, cx| cx.notify());
        });
    }

    pub fn reconcile(&mut self, issue: &Issue, cx: &mut Context<Self>) {
        let top = self.list.logical_scroll_top();
        let old_keys: Vec<_> = self.posts.iter().map(|post| post.key.clone()).collect();
        let mut previous: HashMap<_, _> = std::mem::take(&mut self.posts)
            .into_iter()
            .map(|post| (post.key.clone(), post))
            .collect();
        let format = description_format(&issue.body);
        let mut posts = Vec::with_capacity(issue.comments.len() + 1);
        let mut changed = Vec::new();
        posts.push(reconcile_post(
            PostSource {
                key: PostKey::Body,
                author: &issue.author,
                created_at: &issue.created_at,
                body: &issue.body,
                format,
                comment_index: None,
            },
            &mut previous,
            &mut changed,
            1,
            cx,
        ));
        let mut comments: Vec<_> = issue.comments.iter().enumerate().collect();
        comments.sort_by(|(_, left), (_, right)| left.created_at.cmp(&right.created_at));
        for (index, comment) in comments {
            let row = posts.len() + 1;
            posts.push(reconcile_post(
                PostSource {
                    key: PostKey::Comment(comment.id.clone().into()),
                    author: &comment.author,
                    created_at: &comment.created_at,
                    body: &comment.body,
                    format: BodyMarkdownFormat::Original,
                    comment_index: Some(index),
                },
                &mut previous,
                &mut changed,
                row,
                cx,
            ));
        }
        let metadata_changed = self.title != issue.title || self.state != issue.state;
        self.title.clone_from(&issue.title);
        self.state = issue.state;
        self.posts = posts;
        if metadata_changed
            || !changed.is_empty()
            || old_keys.len() != self.posts.len()
            || old_keys
                .iter()
                .zip(&self.posts)
                .any(|(key, post)| key != &post.key)
        {
            self.scope.generation.set(self.generation().wrapping_add(1));
        }
        self.reconcile_list(&old_keys, top, &changed);
        if metadata_changed {
            self.list.remeasure_items(1..2);
        }
        self.members.clear();
        self.members.extend(self.posts.iter().map(|post| {
            (
                ("issue-conversation-post", post.text.entity_id()).into(),
                post.text.clone(),
            )
        }));
        self.publish_group(cx);
        cx.notify();
    }

    fn reconcile_list(&self, old: &[PostKey], top: ListOffset, changed: &[usize]) {
        let prefix = old
            .iter()
            .zip(&self.posts)
            .take_while(|(key, post)| **key == post.key)
            .count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(self.posts[prefix..].iter().rev())
            .take_while(|(key, post)| **key == post.key)
            .count();
        if prefix + suffix != old.len() || prefix + suffix != self.posts.len() {
            self.list.splice(
                1 + prefix..1 + old.len() - suffix,
                self.posts.len() - prefix - suffix,
            );
            // The comments label belongs to the first reply and may move.
            self.list.remeasure_items(1..self.posts.len() + 1);
            let item_ix = if top.item_ix == 0 {
                0
            } else if top.item_ix > old.len() {
                self.posts.len() + 1
            } else {
                let anchor = top.item_ix - 1;
                old[anchor..]
                    .iter()
                    .chain(old[..anchor].iter().rev())
                    .find_map(|key| self.posts.iter().position(|post| &post.key == key))
                    .map_or(0, |index| index + 1)
            };
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: top.offset_in_item,
            });
        }
        for &row in changed {
            self.list.remeasure_items(row..row + 1);
        }
        self.list.remeasure_items(0..1);
        self.list
            .remeasure_items(self.posts.len() + 1..self.posts.len() + 2);
    }

    pub fn sync_placeholder(&mut self, cx: &mut Context<Self>) {
        let Some(post) = self.posts.first_mut() else {
            return;
        };
        if !matches!(post.prepared.format, BodyMarkdownFormat::Placeholder { .. }) {
            return;
        }
        let format = BodyMarkdownFormat::Placeholder {
            text: Msg::IssueNoDescription.t(),
            italic: false,
        };
        if post.prepared.format == format {
            return;
        }
        post.prepared = PreparedBody::new(&post.prepared.raw, format);
        post.source_revision = post.source_revision.wrapping_add(1);
        post.text
            .update(cx, |text, cx| text.set_text(&post.prepared.text, cx));
        self.list.remeasure_items(1..2);
    }
}

fn description_format(body: &str) -> BodyMarkdownFormat {
    if body.trim().is_empty() {
        BodyMarkdownFormat::Placeholder {
            text: Msg::IssueNoDescription.t(),
            italic: false,
        }
    } else {
        BodyMarkdownFormat::Original
    }
}

struct PostSource<'a> {
    key: PostKey,
    author: &'a str,
    created_at: &'a str,
    body: &'a str,
    format: BodyMarkdownFormat,
    comment_index: Option<usize>,
}

fn reconcile_post(
    source: PostSource<'_>,
    previous: &mut HashMap<PostKey, Post>,
    changed: &mut Vec<usize>,
    row: usize,
    cx: &mut Context<IssueConversation>,
) -> Post {
    let PostSource {
        key,
        author,
        created_at,
        body,
        format,
        comment_index,
    } = source;
    if let Some(mut post) = previous.remove(&key) {
        if post.prepared.raw != body || post.prepared.format != format {
            post.prepared = PreparedBody::new(body, format);
            post.source_revision = post.source_revision.wrapping_add(1);
            post.text
                .update(cx, |text, cx| text.set_text(&post.prepared.text, cx));
            changed.push(row);
        }
        if post.author != author || post.created_at != created_at {
            changed.push(row);
        }
        post.author.clear();
        post.author.push_str(author);
        post.created_at.clear();
        post.created_at.push_str(created_at);
        post.comment_index = comment_index;
        post
    } else {
        let prepared = PreparedBody::new(body, format);
        let text = cx.new(|cx| TextViewState::markdown(&prepared.text, cx).selectable(true));
        changed.push(row);
        Post {
            key,
            author: author.to_owned(),
            created_at: created_at.to_owned(),
            comment_index,
            prepared,
            text,
            source_revision: 1,
        }
    }
}

impl KagiApp {
    pub(super) fn sync_issue_conversation_activation(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(session), Some(number), Some(base)) = (
            self.active_session(),
            self.ui().selected_github_issue,
            self.ui().issue_composer.base_repo.as_ref(),
        ) else {
            return;
        };
        if self.selected_issue_conversation().is_some()
            || !self.ui().github_issue_details.contains_key(&number)
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let base = base.clone();
        let consumer = self.ui().issue_conversation_gen;
        let detail = self.ui().github_issue_detail_gen;
        // Activation may select an already accepted cache without a new read.
        // Preparation runs after render leases, never while walking the view.
        window.defer(cx, move |_, cx| {
            let _ = weak.update(cx, |app, cx| {
                if app.active_session() != Some(session)
                    || app.ui().selected_github_issue != Some(number)
                    || app.ui().issue_conversation_gen != consumer
                    || app.ui().github_issue_detail_gen != detail
                    || app.ui().issue_composer.base_repo.as_ref() != Some(&base)
                {
                    return;
                }
                app.ensure_issue_conversation_for(session, number, cx);
                app.activate_issue_conversation(cx);
                cx.notify();
            });
        });
    }

    pub(super) fn ensure_issue_conversation_for(
        &mut self,
        session: crate::app::SessionId,
        number: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.ui.get_mut(&session) else {
            return;
        };
        let Some(base) = ui.issue_composer.base_repo.clone() else {
            return;
        };
        let conversations = ui.issue_conversations.entry(base).or_default();
        let fresh = !conversations.contains_key(&number);
        let conversation = conversations
            .entry(number)
            .or_insert_with(|| cx.new(|cx| IssueConversation::new(number, cx)));
        if ui.selected_github_issue == Some(number) {
            let scope = conversation.read(cx).scope.clone();
            ui.bind_issue_conversation_scope(scope);
        }
        if fresh {
            self.reconcile_issue_conversation_for(session, number, cx);
        }
    }

    pub(super) fn retire_issue_conversation(&mut self, cx: &mut App) {
        let Some(ui) = self.ui_mut() else {
            return;
        };
        for conversation in ui.issue_conversations.values().flat_map(HashMap::values) {
            conversation.update(cx, |owner, cx| owner.retire(cx));
        }
        ui.retire_issue_conversation_scope();
    }

    pub(super) fn selected_issue_conversation(&self) -> Option<Entity<IssueConversation>> {
        let ui = self.ui();
        let base = ui.issue_composer.base_repo.as_deref()?;
        ui.issue_conversations
            .get(base)?
            .get(&ui.selected_github_issue?)
            .cloned()
    }

    pub(super) fn reconcile_issue_conversation_for(
        &mut self,
        session: crate::app::SessionId,
        number: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.ui.get_mut(&session) else {
            return;
        };
        let Some(base) = ui.issue_composer.base_repo.clone() else {
            return;
        };
        let Some(issue) = ui.github_issue_details.get(&number) else {
            return;
        };
        let conversation = ui
            .issue_conversations
            .entry(base)
            .or_default()
            .entry(number)
            .or_insert_with(|| cx.new(|cx| IssueConversation::new(number, cx)));
        conversation.update(cx, |owner, cx| owner.reconcile(issue, cx));
        if ui.selected_github_issue == Some(number) {
            let scope = conversation.read(cx).scope.clone();
            ui.bind_issue_conversation_scope(scope);
        }
    }

    pub(super) fn activate_issue_conversation(&mut self, cx: &mut App) {
        if self.home_in_front()
            || self.has_modal_or_visible_plan(cx)
            || self.menu_overlay.is_some()
            || self.conflict_body_visible()
            || self.workspace_mode() != super::workspace_mode::WorkspaceMode::Issues
        {
            return;
        }
        if self.ui().selected_github_issue.is_some_and(|number| {
            self.ui()
                .issue_composer
                .editors
                .get(&Some(number))
                .is_some_and(|editor| editor.focused)
        }) {
            return;
        }
        if let Some(conversation) = self.selected_issue_conversation() {
            conversation.update(cx, |owner, cx| owner.activate(cx));
        }
    }

    pub(crate) fn accept_github_issue_detail_for(
        &mut self,
        session: crate::app::SessionId,
        generation: u64,
        number: u64,
        result: Result<Issue, kagi_git::github::PrFetchError>,
        cx: &mut Context<Self>,
    ) {
        let success = result.is_ok();
        let accepted = self
            .ui
            .get_mut(&session)
            .is_some_and(|ui| ui.finish_github_issue_detail_request(generation, number, result));
        if !accepted {
            return;
        }
        if success {
            self.reconcile_issue_conversation_for(session, number, cx);
        }
        if self.active_session() == Some(session) {
            self.activate_issue_conversation(cx);
            cx.notify();
        }
    }
}
