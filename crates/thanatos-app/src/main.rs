fn main() {
    gpui_platform::application().run(|cx| {
        hades_ui::init(cx, hades_ui::ThemeMode::Light);
        thanatos_app::abrir_janela(cx);
    });
}
