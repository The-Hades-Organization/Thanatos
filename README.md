# Thanatos

Thanatos: ferramenta do Hades, reescrita em Rust.

## Crates

- `crates/thanatos-core`: regra de negócio, sem interface.
- `crates/thanatos-cli`: binário `thanatos` (clap).
- `crates/thanatos-app`: view gpui exportada e binário `thanatos-app`.

## Estado

CLI e app portados; o app é embutível no painel do Hades (`FerramentaEmbutida`, Hades-UI v0.3.0).

## Relação com o Hades-Skill

Faz parte da reescrita em Rust das ferramentas do [Hades-Skill](https://github.com/The-Hades-Organization/Hades-Skill). O Hades-Skill continua sendo a fonte até o corte, quando passa a chamar o binário.

## Licença

MIT. Veja `LICENSE`.
