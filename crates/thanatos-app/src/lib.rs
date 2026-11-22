//! View gpui do Thanatos, exportada para o app próprio e para o painel do Hades.

mod service;
mod view;
pub mod vm;

pub use view::{ThanatosView, abrir_janela};

pub const NOME: &str = "Thanatos";
