use std::borrow::Cow;

const TAB_REPLACEMENT: &str = "    ";

pub(super) fn normalize(text: &str) -> Cow<'_, str> {
    if !text.contains(['\r', '\t']) {
        return Cow::Borrowed(text);
    }
    let mut normalized = String::with_capacity(text.len());
    let mut pending_carriage_return = false;
    append(&mut normalized, &mut pending_carriage_return, text);
    Cow::Owned(normalized)
}

pub(super) fn append(
    output: &mut String,
    pending_carriage_return: &mut bool,
    delta: &str,
) -> usize {
    if !*pending_carriage_return && !delta.contains(['\r', '\t']) {
        output.push_str(delta);
        return delta.bytes().filter(|byte| *byte == b'\n').count();
    }

    let mut line_breaks = 0usize;
    for character in delta.chars() {
        if *pending_carriage_return {
            match character {
                '\r' => continue,
                '\n' => {
                    output.push('\n');
                    line_breaks += 1;
                    *pending_carriage_return = false;
                    continue;
                }
                _ => clear_current_line(output),
            }
            *pending_carriage_return = false;
        }

        match character {
            '\r' => *pending_carriage_return = true,
            '\n' => {
                output.push('\n');
                line_breaks += 1;
            }
            '\t' => output.push_str(TAB_REPLACEMENT),
            character => output.push(character),
        }
    }
    line_breaks
}

fn clear_current_line(output: &mut String) {
    output.truncate(output.rfind('\n').map_or(0, |index| index + 1));
}
