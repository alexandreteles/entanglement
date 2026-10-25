pub trait Bound {}
pub trait Requirement<T: Bound> { type Item; const LIMIT: usize; fn execute(&self) -> Self::Item; }
pub struct Item;
impl Bound for Item {}
pub trait Child: Requirement<Item> {}
