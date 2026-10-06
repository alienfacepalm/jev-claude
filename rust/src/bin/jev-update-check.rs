//! Background update check (SPEC 14). With no repository root it does nothing.
fn main() {
    jev_router::init();
    let Some(root) = jev_router::repo::root() else { return };
    let state = jev_router::update::check_for_update_now(root);
    jev_router::update::write_state(&state, &jev_router::update::UPDATE_FILE);
}
