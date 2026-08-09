use super::*;

#[test]
fn array_methods() {
    assert_eq!(
        eval_full("var arr = [1, 2, 3]; arr.length"),
        JsValue::Number(3.0)
    );
    assert_eq!(eval_full("[1,2,3].indexOf(2)"), JsValue::Number(1.0));
    assert_eq!(eval_full("[1,2,3].includes(3)"), JsValue::Boolean(true));
}

#[test]
fn array_mutation_persists_on_receiver() {
    // push/pop/shift/unshift/reverse/sort/splice/fill mutate the source variable in place.
    assert_eq!(
        eval_full("var arr = [1, 2]; arr.push(3); arr.length"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("var arr = [1, 2]; arr.push(3); arr[2]"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("var arr = [1, 2, 3]; arr.pop(); arr.length"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var arr = [1, 2, 3]; arr.shift(); arr[0]"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var arr = [2, 3]; arr.unshift(1); arr[0]"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("var arr = [2, 3]; arr.unshift(0, 1); arr.length"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("var arr = [1, 2, 3]; arr.reverse(); arr[0]"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("var arr = [0, 0, 0]; arr.fill(7); arr[1]"),
        JsValue::Number(7.0)
    );
    assert_eq!(
        eval_full("var arr = [0, 0, 0, 0]; arr.fill(7, 1, 3); arr[3]"),
        JsValue::Number(0.0)
    );
}

#[test]
fn array_mutation_persists_on_member_and_this() {
    // Mutations through a member target (obj.items.push) persist on the object.
    assert_eq!(
        eval_full("var obj = { items: [1, 2] }; obj.items.push(3); obj.items.length"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("var obj = { items: [1, 2, 3] }; obj.items.pop(); obj.items.length"),
        JsValue::Number(2.0)
    );
    // Mutations through `this` inside a method persist on the receiver.
    assert_eq!(
        eval_full(
            "var o = { xs: [1], add: function(v) { this.xs.push(v); } }; o.add(2); o.xs.length"
        ),
        JsValue::Number(2.0)
    );
    // Mutations through an indexed target (rows[0].push) persist.
    assert_eq!(
        eval_full("var rows = [[1], [2]]; rows[0].push(9); rows[0].length"),
        JsValue::Number(2.0)
    );
}

#[test]
fn array_sort_default_and_comparator() {
    // Default sort is lexicographic on string form.
    assert_eq!(
        eval_full("var a = [3, 1, 2]; a.sort(); a[0]"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("var a = [10, 2, 1]; a.sort(); a[0]"),
        JsValue::Number(1.0)
    );
    // Numeric comparator sorts ascending by value.
    assert_eq!(
        eval_full("var a = [10, 2, 1]; a.sort(function(x, y) { return x - y; }); a[0]"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("var a = [10, 2, 1]; a.sort(function(x, y) { return x - y; }); a[2]"),
        JsValue::Number(10.0)
    );
    // Descending comparator.
    assert_eq!(
        eval_full("var a = [1, 2, 3]; a.sort(function(x, y) { return y - x; }); a[0]"),
        JsValue::Number(3.0)
    );
}

#[test]
fn array_splice_removes_and_inserts() {
    // splice returns removed elements and mutates the receiver.
    assert_eq!(
        eval_full("var a = [1, 2, 3, 4]; a.splice(1, 2); a.length"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var a = [1, 2, 3, 4]; var r = a.splice(1, 2); r[0]"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var a = [1, 4]; a.splice(1, 0, 2, 3); a[2]"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("var a = [1, 2, 3]; a.splice(-1, 1); a.length"),
        JsValue::Number(2.0)
    );
}

#[test]
fn array_find_index_variants() {
    assert_eq!(
        eval_full("[5, 12, 8, 130].findIndex(function(x) { return x > 10; })"),
        JsValue::Number(1.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3].findIndex(function(x) { return x > 10; })"),
        JsValue::Number(-1.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3, 4].findLast(function(x) { return x < 3; })"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3, 4].findLastIndex(function(x) { return x < 3; })"),
        JsValue::Number(1.0)
    );
}

#[test]
fn array_at_and_last_index_of() {
    assert_eq!(eval_full("[10, 20, 30].at(0)"), JsValue::Number(10.0));
    assert_eq!(eval_full("[10, 20, 30].at(-1)"), JsValue::Number(30.0));
    assert_eq!(eval_full("[10, 20, 30].at(5)"), JsValue::Undefined);
    assert_eq!(
        eval_full("[1, 2, 3, 2, 1].lastIndexOf(2)"),
        JsValue::Number(3.0)
    );
    assert_eq!(eval_full("[1, 2, 3].lastIndexOf(9)"), JsValue::Number(-1.0));
}

#[test]
fn array_flat_map_and_reduce_right() {
    assert_eq!(
        eval_full("[1, 2, 3].flatMap(function(x) { return [x, x * 2]; }).length"),
        JsValue::Number(6.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3].flatMap(function(x) { return [x, x * 2]; })[3]"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("['a', 'b', 'c'].reduceRight(function(acc, x) { return acc + x; })"),
        JsValue::String("cba".into())
    );
    assert_eq!(
        eval_full("[1, 2, 3].reduceRight(function(acc, x) { return acc + x; }, 10)"),
        JsValue::Number(16.0)
    );
}

#[test]
fn array_map_filter() {
    assert_eq!(
        eval_full("[1,2,3,4].filter((x) => x > 2).length"),
        JsValue::Number(2.0)
    );
}

#[test]
fn array_flat_depth_and_copy_within() {
    // Default depth of 1 flattens a single level.
    assert_eq!(
        eval_full("[1, [2, [3]]].flat().length"),
        JsValue::Number(3.0)
    );
    // Explicit depth 2 reaches the inner array.
    assert_eq!(
        eval_full("[1, [2, [3]]].flat(2).length"),
        JsValue::Number(3.0)
    );
    assert_eq!(eval_full("[1, [2, [3]]].flat(2)[2]"), JsValue::Number(3.0));
    // A large depth flattens fully regardless of nesting.
    assert_eq!(
        eval_full("[1, [2, [3, [4]]]].flat(10).length"),
        JsValue::Number(4.0)
    );
    // copyWithin shifts a slice in place without changing length.
    assert_eq!(
        eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3).length"),
        JsValue::Number(5.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3)[0]"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3, 4, 5].copyWithin(0, 3)[1]"),
        JsValue::Number(5.0)
    );
}

#[test]
fn array_non_mutating_change_methods() {
    // toReversed returns a new array and leaves the source unchanged.
    assert_eq!(
        eval_full("var a = [1, 2, 3]; var b = a.toReversed(); b[0] * 10 + a[0]"),
        JsValue::Number(31.0)
    );
    // toSorted orders a copy without mutating the receiver.
    assert_eq!(eval_full("var a = [3, 1, 2]; var b = a.toSorted(function(x, y) { return x - y; }); b[0] * 10 + a[0]"), JsValue::Number(13.0));
    // with replaces one index in a copy, supporting negative indices.
    assert_eq!(eval_full("[1, 2, 3].with(1, 9)[1]"), JsValue::Number(9.0));
    assert_eq!(eval_full("[1, 2, 3].with(-1, 9)[2]"), JsValue::Number(9.0));
    // toSpliced returns a new array with elements removed and inserted.
    assert_eq!(
        eval_full("[1, 2, 3, 4].toSpliced(1, 2, 9).length"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("[1, 2, 3, 4].toSpliced(1, 2, 9)[1]"),
        JsValue::Number(9.0)
    );
}

#[test]
fn array_includes_same_value_zero_and_from_index() {
    // SameValueZero finds NaN, which indexOf-style === cannot.
    assert_eq!(
        eval_full("[1, NaN, 3].includes(NaN)"),
        JsValue::Boolean(true)
    );
    // fromIndex skips earlier matches.
    assert_eq!(
        eval_full("[1, 2, 1].includes(1, 1)"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("[1, 2, 3].includes(1, 1)"),
        JsValue::Boolean(false)
    );
    // Negative fromIndex counts from the end.
    assert_eq!(
        eval_full("[5, 6, 7].includes(5, -1)"),
        JsValue::Boolean(false)
    );
    assert_eq!(
        eval_full("[5, 6, 7].includes(7, -1)"),
        JsValue::Boolean(true)
    );
}

#[test]
fn array_index_of_from_index() {
    // indexOf honours a positive fromIndex.
    assert_eq!(eval_full("[1, 2, 1].indexOf(1, 1)"), JsValue::Number(2.0));
    // Negative fromIndex counts from the end.
    assert_eq!(eval_full("[1, 2, 1].indexOf(1, -1)"), JsValue::Number(2.0));
    // lastIndexOf scans backward and honours fromIndex.
    assert_eq!(eval_full("[1, 2, 1].lastIndexOf(1)"), JsValue::Number(2.0));
    assert_eq!(
        eval_full("[1, 2, 1].lastIndexOf(1, 1)"),
        JsValue::Number(0.0)
    );
    // Absent element yields -1.
    assert_eq!(eval_full("[1, 2, 3].indexOf(9)"), JsValue::Number(-1.0));
}

#[test]
fn array_some_every_pass_index_to_callback() {
    // some/every callbacks receive the element index as the second argument.
    assert_eq!(
        eval_full("[10, 20, 30].some(function(v, i) { return i === 2; })"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("[10, 20, 30].every(function(v, i) { return i < 3; })"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("[10, 20, 30].every(function(v, i) { return i < 2; })"),
        JsValue::Boolean(false)
    );
}

#[test]
fn array_join_null_undefined_and_separator() {
    // null and undefined elements render as empty strings.
    assert_eq!(
        eval_full("[1, null, 2, undefined, 3].join('-')"),
        JsValue::String("1--2--3".to_string())
    );
    // An explicit undefined separator falls back to a comma.
    assert_eq!(
        eval_full("[1, 2, 3].join(undefined)"),
        JsValue::String("1,2,3".to_string())
    );
    // A custom separator is used verbatim.
    assert_eq!(
        eval_full("['a', 'b'].join(' | ')"),
        JsValue::String("a | b".to_string())
    );
}

#[test]
fn array_callbacks_receive_array_argument() {
    // map/filter/forEach/find/some/every callbacks get (element, index, array).
    assert_eq!(
        eval_full("[10,20,30].map(function(v, i, arr) { return arr.length; })[0]"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("[5,6,7].filter(function(v, i, arr) { return arr[i] === v; }).length"),
        JsValue::Number(3.0)
    );
    assert_eq!(
        eval_full("[1,2].find(function(v, i, arr) { return arr.length === 2 && v === 2; })"),
        JsValue::Number(2.0)
    );
    // reduce callback gets (acc, val, index, array).
    assert_eq!(
        eval_full("[1,2,3].reduce(function(acc, v, i, arr) { return acc + arr.length; }, 0)"),
        JsValue::Number(9.0)
    );
}

#[test]
fn array_of_and_from_collections() {
    // Array.of wraps its arguments verbatim (unlike Array(n) which sizes).
    assert_eq!(eval_full("Array.of(7, 8, 9).length"), JsValue::Number(3.0));
    assert_eq!(eval_full("Array.of(7)[0]"), JsValue::Number(7.0));
    // Array.from over a Set yields its unique values.
    assert_eq!(
        eval_full("Array.from(new Set([1, 1, 2, 3])).length"),
        JsValue::Number(3.0)
    );
    // Array.from over a Map yields [key, value] pairs.
    assert_eq!(
        eval_full("Array.from(new Map([['a', 1]]))[0][0]"),
        JsValue::String("a".to_string())
    );
    // Array.from over an array-like object walks 0..length.
    assert_eq!(
        eval_full("Array.from({ length: 2, 0: 'x', 1: 'y' })[1]"),
        JsValue::String("y".to_string())
    );
}

#[test]
fn string_methods() {
    assert_eq!(
        eval_full("'hello world'.split(' ').length"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("'Hello'.toLowerCase()"),
        JsValue::String("hello".into())
    );
    assert_eq!(eval_full("'abc'.indexOf('b')"), JsValue::Number(1.0));
}

#[test]
fn string_at_and_code_point_at() {
    assert_eq!(eval_full("'abc'.at(0)"), JsValue::String("a".into()));
    assert_eq!(eval_full("'abc'.at(-1)"), JsValue::String("c".into()));
    assert_eq!(eval_full("'abc'.at(9)"), JsValue::Undefined);
    assert_eq!(eval_full("'A'.codePointAt(0)"), JsValue::Number(65.0));
    assert_eq!(eval_full("'abc'.codePointAt(9)"), JsValue::Undefined);
}

#[test]
fn string_pad_counts_by_char_not_byte() {
    // ASCII padding pads to the requested length with the given fill.
    assert_eq!(
        eval_full("'5'.padStart(3, '0')"),
        JsValue::String("005".to_string())
    );
    assert_eq!(
        eval_full("'5'.padEnd(3, '.')"),
        JsValue::String("5..".to_string())
    );
    // A target shorter than the string returns the string unchanged.
    assert_eq!(
        eval_full("'hello'.padStart(2)"),
        JsValue::String("hello".to_string())
    );
    // Multi-byte content is counted by characters and never sliced mid-codepoint.
    assert_eq!(
        eval_full("'e'.padStart(3, '\u{20ac}')"),
        JsValue::String("\u{20ac}\u{20ac}e".to_string())
    );
    // Multi-byte source string keeps its full content when already at length.
    assert_eq!(
        eval_full("'\u{20ac}\u{20ac}'.padEnd(2, 'x')"),
        JsValue::String("\u{20ac}\u{20ac}".to_string())
    );
}

#[test]
fn string_index_of_returns_char_position() {
    // ASCII positions are unchanged.
    assert_eq!(eval_full("'hello'.indexOf('l')"), JsValue::Number(2.0));
    assert_eq!(eval_full("'hello'.lastIndexOf('l')"), JsValue::Number(3.0));
    assert_eq!(eval_full("'abc'.indexOf('z')"), JsValue::Number(-1.0));
    // After a 3-byte euro sign, 'x' is at char index 1 (not byte index 3).
    assert_eq!(eval_full("'\u{20ac}x'.indexOf('x')"), JsValue::Number(1.0));
    assert_eq!(
        eval_full("'\u{20ac}x\u{20ac}x'.lastIndexOf('x')"),
        JsValue::Number(3.0)
    );
}

#[test]
fn string_length_counts_chars() {
    // ASCII length is unchanged.
    assert_eq!(eval_full("'hello'.length"), JsValue::Number(5.0));
    // A 3-byte euro sign counts as one character, consistent with slice/charAt.
    assert_eq!(eval_full("'\u{20ac}'.length"), JsValue::Number(1.0));
    assert_eq!(eval_full("'a\u{20ac}b'.length"), JsValue::Number(3.0));
    // Length agrees with char indexing: last valid index is length - 1.
    assert_eq!(
        eval_full("var s = 'a\u{20ac}b'; s[s.length - 1]"),
        JsValue::String("b".to_string())
    );
}

#[test]
fn string_split_limit_substr_concat() {
    // split honours the limit argument.
    assert_eq!(
        eval_full("'a,b,c,d'.split(',', 2).length"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("'a,b,c'.split(',')[2]"),
        JsValue::String("c".to_string())
    );
    // substr(start, length) with a positive and a negative start.
    assert_eq!(
        eval_full("'hello'.substr(1, 3)"),
        JsValue::String("ell".to_string())
    );
    assert_eq!(
        eval_full("'hello'.substr(-2)"),
        JsValue::String("lo".to_string())
    );
    // concat joins all arguments after the receiver.
    assert_eq!(
        eval_full("'a'.concat('b', 'c')"),
        JsValue::String("abc".to_string())
    );
}

#[test]
fn string_locale_compare_ordering() {
    // Returns negative, zero, or positive per the JS contract.
    assert_eq!(eval_full("'a'.localeCompare('b')"), JsValue::Number(-1.0));
    assert_eq!(eval_full("'b'.localeCompare('a')"), JsValue::Number(1.0));
    assert_eq!(eval_full("'a'.localeCompare('a')"), JsValue::Number(0.0));
    // Usable as a sort comparator yielding lexical order.
    assert_eq!(
        eval_full("['c', 'a', 'b'].sort(function(x, y) { return x.localeCompare(y); })[0]"),
        JsValue::String("a".to_string())
    );
}

#[test]
fn string_index_of_with_position() {
    // indexOf honours a start position, counted in chars.
    assert_eq!(eval_full("'abcabc'.indexOf('bc', 2)"), JsValue::Number(4.0));
    // lastIndexOf bounds the match start with fromIndex.
    assert_eq!(
        eval_full("'abcabc'.lastIndexOf('bc')"),
        JsValue::Number(4.0)
    );
    assert_eq!(
        eval_full("'abcabc'.lastIndexOf('bc', 3)"),
        JsValue::Number(1.0)
    );
    // Empty needle clamps to string length.
    assert_eq!(eval_full("'abc'.indexOf('', 5)"), JsValue::Number(3.0));
    // Absent needle yields -1.
    assert_eq!(eval_full("'abc'.indexOf('z')"), JsValue::Number(-1.0));
}

#[test]
fn string_includes_starts_ends_with_position() {
    // startsWith honours a start position.
    assert_eq!(
        eval_full("'abcdef'.startsWith('cd', 2)"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("'abcdef'.startsWith('cd', 1)"),
        JsValue::Boolean(false)
    );
    // endsWith treats the string as ending at endPosition.
    assert_eq!(
        eval_full("'abcdef'.endsWith('cd', 4)"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("'abcdef'.endsWith('cd')"),
        JsValue::Boolean(false)
    );
    // includes honours a start position.
    assert_eq!(
        eval_full("'abcabc'.includes('ab', 1)"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("'abcabc'.includes('ab', 4)"),
        JsValue::Boolean(false)
    );
}

#[test]
fn string_split_separator_variants() {
    // An absent separator yields a single-element array with the whole string.
    assert_eq!(eval_full("'abc'.split().length"), JsValue::Number(1.0));
    assert_eq!(
        eval_full("'abc'.split()[0]"),
        JsValue::String("abc".to_string())
    );
    // An empty-string separator splits into individual characters.
    assert_eq!(eval_full("'abc'.split('').length"), JsValue::Number(3.0));
    // A normal separator splits on each occurrence.
    assert_eq!(eval_full("'a,b,c'.split(',').length"), JsValue::Number(3.0));
}

#[test]
fn string_replace_dollar_patterns() {
    // $& inserts the matched substring; $$ yields a literal dollar sign.
    assert_eq!(
        eval_full("'hello'.replace('l', '[$&]')"),
        JsValue::String("he[l]lo".to_string())
    );
    assert_eq!(
        eval_full("'a'.replace('a', '$$')"),
        JsValue::String("$".to_string())
    );
    // $` and $' expand to the text before and after the match.
    assert_eq!(
        eval_full("'abc'.replace('b', '$`|$\\'')"),
        JsValue::String("aa|cc".to_string())
    );
    // replaceAll applies $& to every occurrence.
    assert_eq!(
        eval_full("'a-a'.replaceAll('a', '($&)')"),
        JsValue::String("(a)-(a)".to_string())
    );
}

#[test]
fn string_replace_with_function() {
    // replace with a callback: fn(match, offset, string).
    assert_eq!(
        eval_full("'hello world'.replace('world', function(m) { return m.toUpperCase(); })"),
        JsValue::String("hello WORLD".to_string())
    );
    // replaceAll invokes the callback for every match.
    assert_eq!(
        eval_full("'aaa'.replaceAll('a', function(m, i) { return String(i); })"),
        JsValue::String("012".to_string())
    );
    // No match leaves the string unchanged.
    assert_eq!(
        eval_full("'abc'.replace('z', function(m) { return 'X'; })"),
        JsValue::String("abc".to_string())
    );
}

#[test]
fn string_match_and_search_plain_string() {
    // match with a plain string returns [match] or null.
    assert_eq!(
        eval_full("'hello world'.match('world')[0]"),
        JsValue::String("world".to_string())
    );
    assert_eq!(eval_full("'hello'.match('xyz')"), JsValue::Null);
    // search returns the byte index of the first occurrence or -1.
    assert_eq!(
        eval_full("'hello world'.search('world')"),
        JsValue::Number(6.0)
    );
    assert_eq!(eval_full("'hello'.search('xyz')"), JsValue::Number(-1.0));
}

#[test]
fn string_from_code_point_builds_scalars() {
    // ASCII code points map to their characters and concatenate in order.
    assert_eq!(
        eval_full("String.fromCodePoint(72, 105)"),
        JsValue::String("Hi".to_string())
    );
    // Code points above the BMP produce a single Unicode scalar.
    assert_eq!(
        eval_full("String.fromCodePoint(128512)"),
        JsValue::String("\u{1F600}".to_string())
    );
    // No arguments yields the empty string.
    assert_eq!(
        eval_full("String.fromCodePoint()"),
        JsValue::String(String::new())
    );
}

#[test]
fn number_to_string_radix_and_precision() {
    assert_eq!(
        eval_full("(255).toString(16)"),
        JsValue::String("ff".into())
    );
    assert_eq!(eval_full("(5).toString(2)"), JsValue::String("101".into()));
    assert_eq!(eval_full("(255).toString()"), JsValue::String("255".into()));
    assert_eq!(
        eval_full("(-10).toString(2)"),
        JsValue::String("-1010".into())
    );
    assert_eq!(
        eval_full("(3.14159).toFixed(2)"),
        JsValue::String("3.14".into())
    );
    assert_eq!(
        eval_full("(123.456).toPrecision(4)"),
        JsValue::String("123.5".into())
    );
    assert_eq!(eval_full("(42).valueOf()"), JsValue::Number(42.0));
}

#[test]
fn number_to_string_radix_with_fraction() {
    // Integer radix conversion is unchanged.
    assert_eq!(
        eval_full("(255).toString(16)"),
        JsValue::String("ff".to_string())
    );
    // Fractional parts are now emitted in the target base.
    assert_eq!(
        eval_full("(255.5).toString(16)"),
        JsValue::String("ff.8".to_string())
    );
    assert_eq!(
        eval_full("(0.5).toString(2)"),
        JsValue::String("0.1".to_string())
    );
    // Negative values keep a leading sign.
    assert_eq!(
        eval_full("(-10).toString(2)"),
        JsValue::String("-1010".to_string())
    );
}

#[test]
fn number_to_string_exponential_notation() {
    // Magnitudes with exponents outside [-6, 21] switch to exponential form.
    assert_eq!(
        eval_full("String(1e21)"),
        JsValue::String("1e+21".to_string())
    );
    assert_eq!(
        eval_full("String(1e-7)"),
        JsValue::String("1e-7".to_string())
    );
    assert_eq!(
        eval_full("String(1.5e30)"),
        JsValue::String("1.5e+30".to_string())
    );
    // Exponents within [-6, 21] stay in plain decimal form.
    assert_eq!(
        eval_full("String(1e20)"),
        JsValue::String("100000000000000000000".to_string())
    );
    assert_eq!(
        eval_full("String(1e-6)"),
        JsValue::String("0.000001".to_string())
    );
    assert_eq!(
        eval_full("String(0.001)"),
        JsValue::String("0.001".to_string())
    );
    // Ordinary integers and negatives are unaffected.
    assert_eq!(eval_full("String(123)"), JsValue::String("123".to_string()));
    assert_eq!(
        eval_full("String(-1e21)"),
        JsValue::String("-1e+21".to_string())
    );
}

#[test]
fn number_to_exponential() {
    // The exponent always carries an explicit sign.
    assert_eq!(
        eval_full("(5).toExponential()"),
        JsValue::String("5e+0".to_string())
    );
    assert_eq!(
        eval_full("(12345).toExponential(2)"),
        JsValue::String("1.23e+4".to_string())
    );
    assert_eq!(
        eval_full("(0).toExponential()"),
        JsValue::String("0e+0".to_string())
    );
    assert_eq!(
        eval_full("(0).toExponential(2)"),
        JsValue::String("0.00e+0".to_string())
    );
    // Negative exponents and rounding (half away from zero) with carry.
    assert_eq!(
        eval_full("(0.0001).toExponential()"),
        JsValue::String("1e-4".to_string())
    );
    assert_eq!(
        eval_full("(1.999).toExponential(2)"),
        JsValue::String("2.00e+0".to_string())
    );
    assert_eq!(
        eval_full("(-12345).toExponential(2)"),
        JsValue::String("-1.23e+4".to_string())
    );
}

#[test]
fn number_to_precision() {
    // Fixed notation: exponent in [-6, p).
    assert_eq!(
        eval_full("(5).toPrecision(2)"),
        JsValue::String("5.0".to_string())
    );
    assert_eq!(
        eval_full("(123.456).toPrecision(5)"),
        JsValue::String("123.46".to_string())
    );
    assert_eq!(
        eval_full("(0).toPrecision(1)"),
        JsValue::String("0".to_string())
    );
    assert_eq!(
        eval_full("(0).toPrecision(3)"),
        JsValue::String("0.00".to_string())
    );
    // Exponential notation carries an explicit sign.
    assert_eq!(
        eval_full("(123.456).toPrecision(2)"),
        JsValue::String("1.2e+2".to_string())
    );
    assert_eq!(
        eval_full("(0.0000001).toPrecision(2)"),
        JsValue::String("1.0e-7".to_string())
    );
    // Rounding with carry that bumps the exponent.
    assert_eq!(
        eval_full("(9.99).toPrecision(2)"),
        JsValue::String("10".to_string())
    );
    // Negative values.
    assert_eq!(
        eval_full("(-123.456).toPrecision(2)"),
        JsValue::String("-1.2e+2".to_string())
    );
}

#[test]
fn number_to_fixed_rounds_half_away_from_zero() {
    // JS toFixed rounds halves away from zero, unlike Rust's default formatter.
    assert_eq!(
        eval_full("(2.5).toFixed(0)"),
        JsValue::String("3".to_string())
    );
    assert_eq!(
        eval_full("(0.5).toFixed(0)"),
        JsValue::String("1".to_string())
    );
    assert_eq!(
        eval_full("(-2.5).toFixed(0)"),
        JsValue::String("-3".to_string())
    );
    // Ordinary rounding and padding still hold.
    assert_eq!(
        eval_full("(123.456).toFixed(2)"),
        JsValue::String("123.46".to_string())
    );
    assert_eq!(
        eval_full("(0).toFixed(2)"),
        JsValue::String("0.00".to_string())
    );
}

#[test]
fn math_trig_and_extended_functions() {
    // Trigonometric identities at well-known points.
    assert_eq!(eval_full("Math.cos(0)"), JsValue::Number(1.0));
    assert_eq!(eval_full("Math.sin(0)"), JsValue::Number(0.0));
    // Logarithms base 2 and 10.
    assert_eq!(eval_full("Math.log2(8)"), JsValue::Number(3.0));
    assert_eq!(eval_full("Math.log10(1000)"), JsValue::Number(3.0));
    // Cube root and Euclidean distance.
    assert_eq!(eval_full("Math.cbrt(27)"), JsValue::Number(3.0));
    assert_eq!(eval_full("Math.hypot(3, 4)"), JsValue::Number(5.0));
    // Exponential at zero and inverse tangent quadrant handling.
    assert_eq!(eval_full("Math.exp(0)"), JsValue::Number(1.0));
    assert_eq!(eval_full("Math.atan2(0, 1)"), JsValue::Number(0.0));
    // clz32 counts leading zero bits of the 32-bit representation.
    assert_eq!(eval_full("Math.clz32(1)"), JsValue::Number(31.0));
}

#[test]
fn math_imul_and_hyperbolic_inverses() {
    // imul performs 32-bit integer multiplication with wraparound.
    assert_eq!(eval_full("Math.imul(3, 4)"), JsValue::Number(12.0));
    assert_eq!(eval_full("Math.imul(-5, 3)"), JsValue::Number(-15.0));
    // Large products wrap within the signed 32-bit range.
    assert_eq!(eval_full("Math.imul(0xffffffff, 5)"), JsValue::Number(-5.0));
    // Inverse hyperbolic functions round-trip their forward counterparts.
    assert_eq!(eval_full("Math.asinh(0)"), JsValue::Number(0.0));
    assert_eq!(eval_full("Math.acosh(1)"), JsValue::Number(0.0));
    assert_eq!(eval_full("Math.atanh(0)"), JsValue::Number(0.0));
}

#[test]
fn number_and_math_constants_and_predicates() {
    // Math constants resolve as member access.
    assert_eq!(
        eval_full("Math.PI > 3.14 && Math.PI < 3.15"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("Math.E > 2.71 && Math.E < 2.72"),
        JsValue::Boolean(true)
    );
    // Number constants resolve as member access.
    assert_eq!(
        eval_full("Number.MAX_SAFE_INTEGER"),
        JsValue::Number(9007199254740991.0)
    );
    assert_eq!(
        eval_full("Number.POSITIVE_INFINITY > 1e308"),
        JsValue::Boolean(true)
    );
    // Integer predicates discriminate fractional and non-numeric inputs.
    assert_eq!(eval_full("Number.isInteger(4)"), JsValue::Boolean(true));
    assert_eq!(eval_full("Number.isInteger(4.5)"), JsValue::Boolean(false));
    assert_eq!(eval_full("Number.isInteger('4')"), JsValue::Boolean(false));
    assert_eq!(
        eval_full("Number.isSafeInteger(9007199254740991)"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("Number.isSafeInteger(9007199254740993)"),
        JsValue::Boolean(false)
    );
}

#[test]
fn number_predicates_do_not_coerce() {
    // Number.isNaN/isFinite reject non-numbers without coercion...
    assert_eq!(eval_full("Number.isNaN('foo')"), JsValue::Boolean(false));
    assert_eq!(eval_full("Number.isFinite('42')"), JsValue::Boolean(false));
    // ...while still recognising genuine numeric cases.
    assert_eq!(eval_full("Number.isNaN(NaN)"), JsValue::Boolean(true));
    assert_eq!(eval_full("Number.isFinite(42)"), JsValue::Boolean(true));
    // Global isNaN/isFinite keep their coercing behaviour.
    assert_eq!(eval_full("isNaN('foo')"), JsValue::Boolean(true));
    assert_eq!(eval_full("isFinite('42')"), JsValue::Boolean(true));
}

#[test]
fn global_infinity_and_nan_identifiers() {
    // Bare Infinity resolves to the positive infinity number.
    assert_eq!(eval_full("Infinity > 1e308"), JsValue::Boolean(true));
    assert_eq!(eval_full("-Infinity < -1e308"), JsValue::Boolean(true));
    // NaN resolves to a NaN value (detected via Number.isNaN since NaN !== NaN).
    assert_eq!(eval_full("Number.isNaN(NaN)"), JsValue::Boolean(true));
    // Infinity is usable as a flat() depth to flatten fully.
    assert_eq!(
        eval_full("[1, [2, [3, [4]]]].flat(Infinity).length"),
        JsValue::Number(4.0)
    );
}

#[test]
fn json_stringify_indentation() {
    // Number space indents each nesting level by that many spaces.
    assert_eq!(
        eval_full("JSON.stringify([1, 2], null, 2)"),
        JsValue::String("[\n  1,\n  2\n]".to_string())
    );
    // A single-key object is deterministic and reflects the indent.
    assert_eq!(
        eval_full("JSON.stringify({ a: 1 }, null, 2)"),
        JsValue::String("{\n  \"a\": 1\n}".to_string())
    );
    // String space is used verbatim as the indent unit.
    assert_eq!(
        eval_full("JSON.stringify([1], null, '\\t')"),
        JsValue::String("[\n\t1\n]".to_string())
    );
    // Empty containers stay compact.
    assert_eq!(
        eval_full("JSON.stringify([], null, 2)"),
        JsValue::String("[]".to_string())
    );
    // Omitting the space argument keeps compact output.
    assert_eq!(
        eval_full("JSON.stringify([1, 2])"),
        JsValue::String("[1,2]".to_string())
    );
}

#[test]
fn json_stringify_replacer_array() {
    // Replacer array whitelists object properties.
    assert_eq!(
        eval_full("JSON.stringify({a: 1, b: 2, c: 3}, ['a', 'c'])"),
        JsValue::String("{\"a\":1,\"c\":3}".to_string())
    );
    // Nested objects are also filtered.
    assert_eq!(
        eval_full("JSON.stringify({x: {a: 1, b: 2}}, ['x', 'a'])"),
        JsValue::String("{\"x\":{\"a\":1}}".to_string())
    );
    // Arrays are unaffected by the replacer.
    assert_eq!(
        eval_full("JSON.stringify([1, 2, 3], ['a'])"),
        JsValue::String("[1,2,3]".to_string())
    );
}

#[test]
fn json_stringify_spec_edge_cases() {
    // Non-finite numbers serialize as null.
    assert_eq!(
        eval_full("JSON.stringify(NaN)"),
        JsValue::String("null".to_string())
    );
    assert_eq!(
        eval_full("JSON.stringify([1, Infinity, 2])"),
        JsValue::String("[1,null,2]".to_string())
    );
    // undefined array elements become null.
    assert_eq!(
        eval_full("JSON.stringify([1, undefined, 3])"),
        JsValue::String("[1,null,3]".to_string())
    );
    // undefined object properties are omitted.
    assert_eq!(
        eval_full("JSON.stringify({ a: 1, b: undefined })"),
        JsValue::String("{\"a\":1}".to_string())
    );
    // Control characters in strings are escaped.
    assert_eq!(
        eval_full("JSON.stringify('a\\nb')"),
        JsValue::String("\"a\\nb\"".to_string())
    );
}

#[test]
fn json_parse_top_level_string_escapes() {
    // A top-level JSON string decodes tab and unicode escapes like nested ones.
    assert_eq!(
        eval_full(r#"JSON.parse('"a\\tb"')"#),
        JsValue::String("a\tb".to_string())
    );
    assert_eq!(
        eval_full(r#"JSON.parse('"\\u0041"')"#),
        JsValue::String("A".to_string())
    );
}

#[test]
fn map_basic() {
    assert_eq!(
        eval_full(
            "
        var m = new Map([['a', 1], ['b', 2]]);
        m.get('a')
    "
        ),
        JsValue::Number(1.0)
    );
}

#[test]
fn set_basic() {
    assert_eq!(
        eval_full(
            "
        var s = new Set([1, 2, 3]);
        s.has(2)
    "
        ),
        JsValue::Boolean(true)
    );
}

#[test]
fn map_and_set_mutations_persist() {
    // Map.set persists and get reads it back across statements.
    assert_eq!(
        eval_full("var m = new Map(); m.set('a', 1); m.set('b', 2); m.get('b')"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var m = new Map(); m.set('a', 1); m.set('a', 9); m.get('a')"),
        JsValue::Number(9.0)
    );
    assert_eq!(
        eval_full("var m = new Map([['a', 1]]); m.delete('a'); m.has('a')"),
        JsValue::Boolean(false)
    );
    // Set.add persists and stays unique; delete removes.
    assert_eq!(
        eval_full("var s = new Set(); s.add(1); s.add(1); s.add(2); s.size()"),
        JsValue::Number(2.0)
    );
    assert_eq!(
        eval_full("var s = new Set([1, 2, 3]); s.delete(2); s.has(2)"),
        JsValue::Boolean(false)
    );
    // Mutation through `this` inside a method persists to the receiver.
    assert_eq!(eval_full("var o = { bag: new Set(), put: function(v) { this.bag.add(v); } }; o.put(5); o.put(5); o.bag.size()"), JsValue::Number(1.0));
}

#[test]
fn map_and_set_for_each_invoke_callback() {
    // Map.forEach passes (value, key): summing values yields 1 + 2 + 3 = 6.
    assert_eq!(eval_full("var m = new Map([['a', 1], ['b', 2], ['c', 3]]); var t = 0; m.forEach(function(v) { t = t + v; }); t"), JsValue::Number(6.0));
    // The key is provided as the second argument.
    assert_eq!(
        eval_full(
            "var m = new Map([['x', 10]]); var k = ''; m.forEach(function(v, key) { k = key; }); k"
        ),
        JsValue::String("x".to_string())
    );
    // Set.forEach iterates each unique value.
    assert_eq!(
        eval_full(
            "var s = new Set([2, 4, 6]); var t = 0; s.forEach(function(v) { t = t + v; }); t"
        ),
        JsValue::Number(12.0)
    );
}

#[test]
fn weakmap_and_weakset_support_core_methods() {
    // WeakMap.set/get/has/delete persist like Map.
    assert_eq!(
        eval_full("var w = new WeakMap(); w.set('k', 42); w.get('k')"),
        JsValue::Number(42.0)
    );
    assert_eq!(
        eval_full("var w = new WeakMap([['a', 1]]); w.has('a')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("var w = new WeakMap([['a', 1]]); w.delete('a'); w.has('a')"),
        JsValue::Boolean(false)
    );
    // WeakSet.add/has/delete persist like Set.
    assert_eq!(
        eval_full("var w = new WeakSet(); w.add('x'); w.has('x')"),
        JsValue::Boolean(true)
    );
    assert_eq!(
        eval_full("var w = new WeakSet(['x']); w.delete('x'); w.has('x')"),
        JsValue::Boolean(false)
    );
}

#[test]
fn parse_int_leading_numeric_and_radix() {
    // Stops at the first non-digit, keeping the leading integer.
    assert_eq!(eval_full("parseInt('42px')"), JsValue::Number(42.0));
    // Auto-detects a hex prefix when no radix is given.
    assert_eq!(eval_full("parseInt('0xFF')"), JsValue::Number(255.0));
    // Explicit radix parses in that base.
    assert_eq!(eval_full("parseInt('101', 2)"), JsValue::Number(5.0));
    // Truncates a fractional string to its integer part.
    assert_eq!(eval_full("parseInt('3.99')"), JsValue::Number(3.0));
    // Honours a leading sign and surrounding whitespace.
    assert_eq!(eval_full("parseInt('   -7abc')"), JsValue::Number(-7.0));
    // No digits produces NaN (compared via self-inequality).
    assert_eq!(
        eval_full("parseInt('abc') !== parseInt('abc')"),
        JsValue::Boolean(true)
    );
}

#[test]
fn parse_float_leading_numeric() {
    // Parses the numeric prefix and ignores trailing text.
    assert_eq!(eval_full("parseFloat('2.5abc')"), JsValue::Number(2.5));
    // Supports exponent notation.
    assert_eq!(eval_full("parseFloat('1.5e2xyz')"), JsValue::Number(150.0));
    // Recognises Infinity.
    assert_eq!(
        eval_full("parseFloat('-Infinity')"),
        JsValue::Number(f64::NEG_INFINITY)
    );
    // No numeric prefix produces NaN.
    assert_eq!(
        eval_full("parseFloat('nope') !== parseFloat('nope')"),
        JsValue::Boolean(true)
    );
}

#[test]
fn loose_equality_null_and_undefined() {
    // null and undefined are loosely equal to each other only.
    assert_eq!(eval_full("null == undefined"), JsValue::Boolean(true));
    assert_eq!(eval_full("null == 0"), JsValue::Boolean(false));
    assert_eq!(eval_full("undefined == 0"), JsValue::Boolean(false));
    assert_eq!(eval_full("null == false"), JsValue::Boolean(false));
    assert_eq!(eval_full("null == ''"), JsValue::Boolean(false));
    // != is the negation.
    assert_eq!(eval_full("null != 0"), JsValue::Boolean(true));
}

#[test]
fn to_number_string_coercion() {
    // Empty and whitespace-only strings coerce to 0 (not NaN).
    assert_eq!(eval_full("+''"), JsValue::Number(0.0));
    assert_eq!(eval_full("+'   '"), JsValue::Number(0.0));
    // Surrounding whitespace is ignored around a valid literal.
    assert_eq!(eval_full("+'  42 '"), JsValue::Number(42.0));
    // Non-decimal integer prefixes are honoured.
    assert_eq!(eval_full("+'0x10'"), JsValue::Number(16.0));
    assert_eq!(eval_full("+'0b101'"), JsValue::Number(5.0));
    assert_eq!(eval_full("+'0o17'"), JsValue::Number(15.0));
    // The Infinity literal maps to positive infinity.
    assert_eq!(eval_full("+'Infinity'"), JsValue::Number(f64::INFINITY));
    // Non-numeric strings (including Rust-only spellings) are NaN.
    assert_eq!(eval_full("Number.isNaN(+'abc')"), JsValue::Boolean(true));
    assert_eq!(eval_full("Number.isNaN(+'inf')"), JsValue::Boolean(true));
    assert_eq!(eval_full("Number.isNaN(+'nan')"), JsValue::Boolean(true));
}

#[test]
fn wrapper_constructors_coerce_as_functions() {
    // Number() coerces its argument (and defaults to 0 with no argument).
    assert_eq!(eval_full("Number('42')"), JsValue::Number(42.0));
    assert_eq!(eval_full("Number(true)"), JsValue::Number(1.0));
    assert_eq!(eval_full("Number()"), JsValue::Number(0.0));
    // String() renders the argument as a string (empty when omitted).
    assert_eq!(eval_full("String(123)"), JsValue::String("123".to_string()));
    assert_eq!(
        eval_full("String(null)"),
        JsValue::String("null".to_string())
    );
    assert_eq!(eval_full("String()"), JsValue::String(String::new()));
    // Boolean() applies truthiness (false when omitted).
    assert_eq!(eval_full("Boolean('')"), JsValue::Boolean(false));
    assert_eq!(eval_full("Boolean('x')"), JsValue::Boolean(true));
    assert_eq!(eval_full("Boolean()"), JsValue::Boolean(false));
}

#[test]
fn addition_to_primitive_coercion() {
    // Arrays coerce via toString (join with comma) before + decides concat vs add.
    assert_eq!(eval_full("[] + []"), JsValue::String(String::new()));
    assert_eq!(
        eval_full("[1,2] + [3,4]"),
        JsValue::String("1,23,4".to_string())
    );
    assert_eq!(eval_full("[1] + 2"), JsValue::String("12".to_string()));
    // Empty array to number: [] -> '' -> 0.
    assert_eq!(eval_full("+[]"), JsValue::Number(0.0));
    // Objects coerce to [object Object].
    assert_eq!(
        eval_full("var o = {}; o + ''"),
        JsValue::String("[object Object]".to_string())
    );
}

#[test]
fn relational_string_comparison() {
    // When both operands are strings, compare lexicographically.
    assert_eq!(eval_full("'a' < 'b'"), JsValue::Boolean(true));
    assert_eq!(eval_full("'b' < 'a'"), JsValue::Boolean(false));
    assert_eq!(eval_full("'10' < '9'"), JsValue::Boolean(true)); // lexicographic: '1' < '9'
    assert_eq!(eval_full("'abc' <= 'abc'"), JsValue::Boolean(true));
    assert_eq!(eval_full("'z' > 'a'"), JsValue::Boolean(true));
    // Mixed types still compare numerically.
    assert_eq!(eval_full("10 < 9"), JsValue::Boolean(false));
    assert_eq!(eval_full("'10' < 9"), JsValue::Boolean(false)); // '10' -> 10, numeric
}

#[test]
fn array_and_object_to_string() {
    // Array.prototype.toString is join(","), rendering null/undefined as empty.
    assert_eq!(
        eval_full("[1,2,3].toString()"),
        JsValue::String("1,2,3".to_string())
    );
    assert_eq!(eval_full("[].toString()"), JsValue::String(String::new()));
    assert_eq!(
        eval_full("[1,null,undefined,2].toString()"),
        JsValue::String("1,,,2".to_string())
    );
    assert_eq!(
        eval_full("[1,2,3].toLocaleString()"),
        JsValue::String("1,2,3".to_string())
    );
    // Object.prototype.toString tags plain objects.
    assert_eq!(
        eval_full("({}).toString()"),
        JsValue::String("[object Object]".to_string())
    );
}
