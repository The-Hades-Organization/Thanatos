//! View gpui do Thanatos, exportada para o app próprio e para o painel do Hades.

mod service;
mod view;
pub mod vm;

pub use service::{ConfigDoCore, FonteConfig};
pub use view::{ThanatosView, abrir_janela, abrir_janela_em};

pub const NOME: &str = "Thanatos";
