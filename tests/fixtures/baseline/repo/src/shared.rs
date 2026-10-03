pub fn ping() {}

fn local_ping() {
    ping();
    crate::api::serve();
}
