use hades_ui::ThemeMode;

fn main() {
    let modo = if std::env::args().any(|a| a == "--dark") {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    gpui_platform::application().run(move |cx| {
        hades_ui::init(cx, modo);
        thanatos_app::abrir_janela(cx);
    });
}
