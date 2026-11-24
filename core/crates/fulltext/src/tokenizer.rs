//! Words for the index: Chinese is split into words with jieba (in search
//! mode, which also yields the shorter words inside long ones), everything
//! else at spaces and punctuation; all lowercased.

use std::sync::{Arc, LazyLock};

use jieba_rs::{Jieba, TokenizeMode};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

static JIEBA: LazyLock<Arc<Jieba>> = LazyLock::new(|| Arc::new(Jieba::new()));

#[derive(Clone, Default)]
pub struct WordTokenizer;

pub struct WordStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for WordStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

impl Tokenizer for WordTokenizer {
    type TokenStream<'a> = WordStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> WordStream {
        WordStream { tokens: tokens(text), index: 0 }
    }
}

/// The words of a text with their byte ranges.
pub fn tokens(text: &str) -> Vec<Token> {
    // jieba reports character positions; map them to bytes.
    let mut bytes: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    bytes.push(text.len());
    let mut found = Vec::new();
    for word in JIEBA.tokenize(text, TokenizeMode::Search, true) {
        let piece = word.word;
        if !piece.chars().any(char::is_alphanumeric) {
            continue;
        }
        let (from, to) = (bytes[word.start], bytes[word.end]);
        // jieba keeps runs like "deep-learning" or "x86_64" whole; index the
        // parts too so either finds it.
        let mut parts: Vec<(usize, &str)> = Vec::new();
        let mut start: Option<usize> = None;
        for (i, c) in piece.char_indices() {
            if c.is_alphanumeric() {
                start.get_or_insert(i);
            } else if let Some(s) = start.take() {
                parts.push((s, &piece[s..i]));
            }
        }
        if let Some(s) = start {
            parts.push((s, &piece[s..]));
        }
        if parts.len() > 1 {
            for (offset, part) in parts {
                push(&mut found, part, from + offset, from + offset + part.len());
            }
        } else {
            push(&mut found, piece, from, to);
        }
    }
    found
}

fn push(found: &mut Vec<Token>, word: &str, from: usize, to: usize) {
    found.push(Token {
        offset_from: from,
        offset_to: to,
        position: found.len(),
        text: word.to_lowercase(),
        position_length: 1,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokens(text).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn splits_chinese_into_words() {
        let found = words("基于深度学习的文献分类");
        assert!(found.contains(&"深度".to_string()) && found.contains(&"学习".to_string()), "{found:?}");
        assert!(found.contains(&"文献".to_string()), "{found:?}");
    }

    #[test]
    fn keeps_latin_words_and_offsets() {
        let text = "Attention Is All You Need, deep-learning!";
        let found = tokens(text);
        let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.contains(&"attention") && texts.contains(&"need"), "{texts:?}");
        assert!(texts.contains(&"deep") && texts.contains(&"learning"), "{texts:?}");
        for token in &found {
            assert_eq!(text[token.offset_from..token.offset_to].to_lowercase(), token.text);
        }
    }
}
