# Thanatos

Thanatos: ferramenta do Hades, reescrita em Rust.

## Crates

- `crates/thanatos-core`: regra de negócio, sem interface.
- `crates/thanatos-cli`: binário `thanatos` (clap).
- `crates/thanatos-app`: view gpui exportada e binário `thanatos-app`.

## Estado

F0, em construção. Só o esqueleto do workspace existe; nada da ferramenta foi portado ainda.

## Relação com o Hades-Skill

Faz parte da reescrita em Rust das ferramentas do [Hades-Skill](https://github.com/The-Hades-Organization/Hades-Skill). O Hades-Skill continua sendo a fonte até o corte, quando passa a chamar o binário.

## Licença

MIT. Veja `LICENSE`.
