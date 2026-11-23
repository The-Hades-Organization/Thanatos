//! Página Configurações: linhas das chaves `HADES_THANATOS_*`, rascunho, validação e diferença a gravar, sem janela
//! nem I/O. Os valores vêm e vão pelo `hades-core::config` (ver `service::FonteConfig`).

pub const PREFIXO: &str = "HADES_THANATOS_";

/// Tipo da coluna 3 do `config.tsv` (`validate`, `src/lib/config-catalog.ts:138`); só os usados pelo Thanatos têm
/// controle próprio, o resto cai em texto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Bool,
    Int { min: i64, max: i64 },
    Texto,
}

impl Tipo {
    pub fn de_catalogo(kind: &str) -> Tipo {
        if kind == "bool" {
            return Tipo::Bool;
        }
        let faixa = kind.strip_prefix("int:").and_then(|f| {
            let (min, max) = f.split_once(':')?;
            Some((min.parse().ok()?, max.parse().ok()?))
        });
        match faixa {
            Some((min, max)) => Tipo::Int { min, max },
            None => Tipo::Texto,
        }
    }

    /// Valor no formato em que o CLI grava (`config-catalog.ts:147-153`), ou a mensagem de erro.
    pub fn normalizar(self, texto: &str) -> Result<String, String> {
        let v = texto.trim();
        match self {
            Tipo::Bool => match v.to_lowercase().as_str() {
                "1" | "true" | "on" | "sim" => Ok("1".to_owned()),
                "0" | "false" | "off" | "não" => Ok("0".to_owned()),
                _ => Err("Use 0 ou 1".to_owned()),
            },
            Tipo::Int { min, max } => {
                let digitos = v.strip_prefix('-').unwrap_or(v);
                let valido = !digitos.is_empty() && digitos.bytes().all(|b| b.is_ascii_digit());
                let faixa = format!("Número inteiro entre {min} e {max}");
                if !valido {
                    return Err(faixa);
                }
                match v.parse::<i64>() {
                    Ok(n) if (min..=max).contains(&n) => Ok(n.to_string()),
                    _ => Err(faixa),
                }
            }
            Tipo::Texto => Ok(v.to_owned()),
        }
    }
}

/// Onde o valor mostrado está definido. Ambiente vence arquivo, que vence o padrão (`effectiveConfig`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origem {
    Padrao,
    Arquivo,
    Ambiente,
}

/// Uma chave do catálogo com o que está salvo e o que o ambiente impõe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chave {
    pub chave: String,
    pub rotulo: String,
    pub descricao: String,
    pub padrao: String,
    pub tipo: Tipo,
    /// Valor do `config.env`, ou o padrão se a chave não está lá.
    pub salvo: String,
    /// Valor da variável de ambiente, vazia inclusive.
    pub ambiente: Option<String>,
}

impl Chave {
    pub fn origem(&self) -> Origem {
        if self.ambiente.is_some() {
            Origem::Ambiente
        } else if self.salvo != self.padrao {
            Origem::Arquivo
        } else {
            Origem::Padrao
        }
    }

    /// O que o CLI enxerga (`effectiveConfig`: ambiente > `config.env` > padrão).
    pub fn efetivo(&self) -> &str {
        self.ambiente.as_deref().unwrap_or(&self.salvo)
    }
}

/// O que gravar: valores novos e chaves a devolver ao padrão (`save(store, updates, unset)`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mudancas {
    pub atualizar: Vec<(String, String)>,
    pub remover: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Linha {
    pub chave: Chave,
    pub rascunho: String,
}

#[derive(Clone, Debug, Default)]
pub struct Pagina {
    linhas: Vec<Linha>,
}

impl Pagina {
    pub fn new(chaves: Vec<Chave>) -> Self {
        let linhas = chaves
            .into_iter()
            .map(|chave| Linha {
                rascunho: chave.efetivo().to_owned(),
                chave,
            })
            .collect();
        Pagina { linhas }
    }

    pub fn linhas(&self) -> &[Linha] {
        &self.linhas
    }

    pub fn definir(&mut self, i: usize, texto: &str) {
        if let Some(l) = self.linhas.get_mut(i)
            && l.chave.ambiente.is_none()
        {
            texto.clone_into(&mut l.rascunho);
        }
    }

    pub fn restaurar(&mut self, i: usize) {
        if let Some(l) = self.linhas.get_mut(i)
            && l.chave.ambiente.is_none()
        {
            l.chave.padrao.clone_into(&mut l.rascunho);
        }
    }

    pub fn descartar(&mut self) {
        for l in &mut self.linhas {
            l.chave.efetivo().clone_into(&mut l.rascunho);
        }
    }

    pub fn erro(&self, i: usize) -> Option<String> {
        let l = self.linhas.get(i)?;
        if l.chave.ambiente.is_some() {
            return None;
        }
        l.chave.tipo.normalizar(&l.rascunho).err()
    }

    /// Rascunho que difere do salvo (valor inválido conta como pendente: o erro aparece na linha).
    pub fn pendente(&self, i: usize) -> bool {
        let Some(l) = self.linhas.get(i) else {
            return false;
        };
        if l.chave.ambiente.is_some() {
            return false;
        }
        match l.chave.tipo.normalizar(&l.rascunho) {
            Ok(v) => v != l.chave.salvo,
            Err(_) => true,
        }
    }

    pub fn pendentes(&self) -> usize {
        (0..self.linhas.len()).filter(|&i| self.pendente(i)).count()
    }

    pub fn tem_erro(&self) -> bool {
        (0..self.linhas.len()).any(|i| self.erro(i).is_some())
    }

    /// "Restaurar padrão" aparece quando o rascunho já não é o padrão.
    pub fn modificado(&self, i: usize) -> bool {
        self.linhas.get(i).is_some_and(|l| {
            l.chave.ambiente.is_none()
                && l.chave
                    .tipo
                    .normalizar(&l.rascunho)
                    .map_or(true, |v| v != l.chave.padrao)
        })
    }

    pub fn fora_do_padrao(&self) -> usize {
        self.linhas
            .iter()
            .filter(|l| l.chave.efetivo() != l.chave.padrao)
            .count()
    }

    /// A diferença a gravar, ou `None` se algum valor é inválido (nada é gravado).
    pub fn mudancas(&self) -> Option<Mudancas> {
        if self.tem_erro() {
            return None;
        }
        let mut m = Mudancas::default();
        for (i, l) in self.linhas.iter().enumerate() {
            if !self.pendente(i) {
                continue;
            }
            let v = l.chave.tipo.normalizar(&l.rascunho).ok()?;
            if v == l.chave.padrao {
                m.remover.push(l.chave.chave.clone());
            } else {
                m.atualizar.push((l.chave.chave.clone(), v));
            }
        }
        Some(m)
    }
}

pub fn resumo(p: &Pagina) -> String {
    format!(
        "{} configurações · {} fora do padrão",
        p.linhas().len(),
        p.fora_do_padrao()
    )
}
