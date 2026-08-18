use std::rc::Rc;

use super::EventCtx;

type ClickHandler<A> = Rc<dyn Fn(&mut EventCtx<A>)>;

pub struct DeferredAction<A> {
    factory: Rc<dyn Fn() -> A>,
}

impl<A> Clone for DeferredAction<A> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
        }
    }
}

impl<A> DeferredAction<A> {
    pub fn new(factory: impl Fn() -> A + 'static) -> Self {
        Self {
            factory: Rc::new(factory),
        }
    }

    pub fn resolve(&self) -> A {
        (self.factory)()
    }
}

pub struct ClickAction<A> {
    handler: ClickHandler<A>,
}

impl<A> ClickAction<A> {
    pub fn handler(handler: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        Self {
            handler: Rc::new(handler),
        }
    }

    pub fn run(&self, ctx: &mut EventCtx<A>) {
        (self.handler)(ctx);
    }
}

pub trait IntoClickAction<A> {
    fn into_click_action(self) -> ClickAction<A>;
}

impl<A> IntoClickAction<A> for ClickAction<A> {
    fn into_click_action(self) -> ClickAction<A> {
        self
    }
}

impl<A: Clone + 'static> IntoClickAction<A> for A {
    fn into_click_action(self) -> ClickAction<A> {
        ClickAction::handler(move |ctx| ctx.dispatch(self.clone()))
    }
}

impl<A: 'static> IntoClickAction<A> for DeferredAction<A> {
    fn into_click_action(self) -> ClickAction<A> {
        ClickAction::handler(move |ctx| ctx.dispatch(self.resolve()))
    }
}
