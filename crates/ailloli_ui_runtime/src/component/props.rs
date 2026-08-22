//! Marker contract for owned declarative component properties.

/// Cloneable `'static` properties accepted by component nodes.
///
/// A blanket implementation covers every `Clone + 'static` type, so callers
/// should not write a manual implementation. The `'static` bound excludes props
/// that borrow short-lived stack data; use owned values or reference-counted
/// handles instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::Props;
/// #[derive(Clone)]
/// struct LabelProps { text: String }
/// fn accepts_props<P: Props>(props: P) -> P { props }
/// let props = accepts_props(LabelProps { text: "hello".into() });
/// assert_eq!(props.text, "hello");
/// ```
pub trait Props: Clone + 'static {}

/// Implements the Props contract for T where T: Clone + 'static.
impl<T> Props for T where T: Clone + 'static {}
