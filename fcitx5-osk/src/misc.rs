use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
};

pub struct NamedSubscriptionData<T> {
    name: Cow<'static, str>,
    data: T,
}

impl<T> NamedSubscriptionData<T> {
    pub fn new<S>(name: S, data: T) -> Self
    where
        S: Into<Cow<'static, str>>,
    {
        Self {
            name: name.into(),
            data,
        }
    }

    pub fn data(&self) -> &T {
        &self.data
    }
}

impl<T> Hash for NamedSubscriptionData<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
