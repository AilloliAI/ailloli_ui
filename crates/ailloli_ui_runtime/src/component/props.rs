// Example usage in an app:

// #[derive(Clone, Debug)]
// pub struct ChatPageProps {
//     pub workspace_id: WorkspaceId,
//     pub conversation_id: Option<ConversationId>,
// }

// The parameter must be named `props`:

// pub fn chat_page(context: &mut Context<AppAction>, props: ChatPageProps) -> View<AppAction> {
//     ...
// }

pub trait Props: Clone + 'static {}

impl<T> Props for T where T: Clone + 'static {}
