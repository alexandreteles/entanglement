use crate::api::serve as dispatch;

pub fn calls(flag: bool) {
    dispatch();
    let dispatch = || {};
    dispatch();
    crate::api::target();
    if flag {
        crate::api::serve();
    }
}
