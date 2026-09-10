//! The vault itself: initialising one, recovering access to it, who may
//! reach it, and the service routes it serves.

pub(crate) mod initialize_vault;
pub(crate) mod manage_service_routes;
pub(crate) mod manage_users_and_sharing;
pub(crate) mod recover_vault_access;
