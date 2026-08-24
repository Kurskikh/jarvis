use crate::{config, stt, i18n};

pub fn init() -> Result<(), ()> {
    Ok(()) // nothing to init for Vosk
}

pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
    if let Some((recognized, confidence)) = stt::recognize_wake_word(frame_buffer) {
        let recognized = recognized.trim().to_lowercase();
        
        // skip unknown/empty
        if recognized.is_empty() || recognized == "[unk]" {
            return None;
        }
        
        // The score is logged, not enforced - yet.
        //
        // It is thrown away today, and that is the lever missing from the one
        // failure mode left: the detector decoding somebody's ordinary
        // conversation into the wake word. Vosk answers with a summed
        // log-likelihood, so it grows with the length of what was decoded and
        // a threshold picked from taste rather than samples would reject real
        // activations to catch false ones. Logged on both the candidate and
        // the match so real and false wake-ups can be compared and a gate
        // chosen from the numbers.
        info!("Wake word candidate: '{}' (confidence {:.1})", recognized, confidence);
        
        // language-specific wake phrase
        let lang = i18n::get_language();
        let wake_phrases = config::get_wake_phrases(&lang);

        // verify with seqdiff ratio
        for word in recognized.split_whitespace() {
            if word == "[unk]" {
                continue;
            }
            
            let word_chars: Vec<char> = word.chars().collect();
            
            for wake_phrase in wake_phrases {
                let wake_chars: Vec<char> = wake_phrase.chars().collect();
                let similarity = seqdiff::ratio(&wake_chars, &word_chars);
                
                if similarity >= config::VOSK_MIN_RATIO {
                    info!("Wake word match: '{}' ~ '{}' ({:.1}%, confidence {:.1})",
                          word, wake_phrase, similarity, confidence);
                    return Some(0);
                }
            }
        }
        
        // info!("Similarity: {:.1}% ('{}' vs '{}')", similarity, recognized, config::VOSK_FETCH_PHRASE);
    }
    
    None
}

// @TODO. Make it better somehow (more accurate or with higher sensitivity).
// pub fn data_callback(frame_buffer: &[i16]) -> Option<i32> {
//     // recognize & convert to sequence
//     let recognized_phrase = stt::recognize(&frame_buffer, true).unwrap_or("".into());

//     if !recognized_phrase.trim().is_empty() {
//         info!("Vosk wake-word debug info:");
//         info!("rec: {}", recognized_phrase);
//         let recognized_phrases = recognized_phrase.split_whitespace();
//         for phrase in recognized_phrases {
//             let recognized_phrase_chars = phrase.trim().to_lowercase().chars().collect::<Vec<_>>();

//             // compare
//             let compare_ratio = seqdiff::ratio(
//                 &config::VOSK_FETCH_PHRASE.chars().collect::<Vec<_>>(),
//                 &recognized_phrase_chars,
//             );
//             info!("og phrase: {:?}", &config::VOSK_FETCH_PHRASE);
//             info!("recognized phrase: {:?}", &recognized_phrase_chars);
//             info!("compare ratio: {}", compare_ratio);

//             if compare_ratio >= config::VOSK_MIN_RATIO {
//                 info!("Phrase activated.");
//                 return Some(0);
//             }
//         }
//     }

//     None
// }



#[cfg(test)]
mod tests {
    // A false positive from a real microphone: the user said "Джарвис" and
    // "райс" was executed as a command. The obvious fix is to strip anything
    // that looks like the wake word using the same fuzzy match the detector
    // uses - and it does not work, which is worth keeping written down.
    //
    // "райс" scores 40% against the closest wake phrase, well under the 70%
    // the detector demands. It is not a mangled wake word; it is a different
    // word. Sorting it out needs recognition confidence, not string distance.
    #[test]
    fn a_misheard_fragment_is_not_a_mangled_wake_word() {
        let phrases = crate::config::get_wake_phrases("ru");
        let best = |s: &str| -> f64 {
            let sc: Vec<char> = s.chars().collect();
            phrases.iter().map(|p| {
                let pc: Vec<char> = p.chars().collect();
                seqdiff::ratio(&pc, &sc)
            }).fold(0.0, f64::max)
        };

        // the real false positive: far below the gate, so fuzzy stripping
        // would never have caught it
        assert!(best("райс") < crate::config::VOSK_MIN_RATIO,
                "райс scored {:.1}%, so a fuzzy wake-word filter cannot be the fix",
                best("райс"));

        // genuine manglings of the wake word ARE above it, which is why the
        // detector works at all
        for m in ["рвис", "арвис", "гарис", "джервис"] {
            assert!(best(m) >= crate::config::VOSK_MIN_RATIO,
                    "{} scored {:.1}%, expected the detector to accept it", m, best(m));
        }

        // and ordinary speech is nowhere near
        for s in ["как дела", "включи музыку"] {
            assert!(best(s) < 40.0, "{} scored {:.1}%", s, best(s));
        }
    }
}
