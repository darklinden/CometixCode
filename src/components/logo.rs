//! Compatibility shim for the main-screen logo.
//!
//! Maps to: CC `components/LogoV2/LogoV2.tsx`. The official component boundary
//! lives at `components/logo_v2/logo_v2.rs`; this module preserves existing
//! `components::logo::Logo` call sites while `Messages` owns the main-screen
//! logo boundary.

pub(crate) use crate::components::logo_v2::logo_v2::Logo;
