//! Gerador de slug público — o identificador que aparece no link compartilhável.

use rand::seq::SliceRandom;

/// 5 chars, lowercase, sem chars ambíguos (`0/o`, `1/l/i`). ~24 bits.
/// Com UNIQUE constraint + retry no insert, colisões reais são tratadas.
pub fn random_slug() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..5)
        .map(|_| *ALPHABET.choose(&mut rng).unwrap() as char)
        .collect()
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn slug_tem_5_chars() {
        for _ in 0..200 {
            assert_eq!(random_slug().chars().count(), 5);
        }
    }

    #[test]
    fn slug_nao_usa_chars_ambiguos() {
        // 0/o, 1/l/i são os pares que o usuário erra ao digitar um link ditado
        // por voz ou lido de um print.
        let proibidos: HashSet<char> = "01loi".chars().collect();
        for _ in 0..500 {
            let slug = random_slug();
            for c in slug.chars() {
                assert!(
                    !proibidos.contains(&c),
                    "char ambíguo {c:?} apareceu no slug {slug:?}"
                );
                assert!(
                    c.is_ascii_lowercase() || c.is_ascii_digit(),
                    "char inesperado {c:?} no slug {slug:?}"
                );
            }
        }
    }

    #[test]
    fn slug_varia_entre_chamadas() {
        // Não é teste de qualidade de RNG — só pega o caso de constante hardcoded.
        let amostras: HashSet<String> = (0..100).map(|_| random_slug()).collect();
        assert!(
            amostras.len() > 90,
            "esperado ~100 slugs distintos, veio {}",
            amostras.len()
        );
    }
}
