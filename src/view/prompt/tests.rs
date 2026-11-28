use super::*;

fn ask(answers: &str) -> (Choice, String) {
    let mut output = Vec::new();
    let choice = overwrite(
        Path::new("custom/synth/envelope.lua"),
        "attack = 5\n",
        "attack = 15\n",
        false,
        &mut answers.as_bytes(),
        &mut output,
    )
    .unwrap();
    (choice, String::from_utf8(output).unwrap())
}

#[test]
fn y_overwrites_and_n_keeps() {
    assert_eq!(ask("y\n").0, Choice::Overwrite);
    assert_eq!(ask("YES\n").0, Choice::Overwrite);
    assert_eq!(ask("n\n").0, Choice::Keep);
}

#[test]
fn d_shows_the_difference_then_asks_again() {
    let (choice, output) = ask("d\ny\n");
    assert_eq!(choice, Choice::Overwrite);
    assert!(output.contains("- attack = 5\n+ attack = 15\n"), "{output}");
    assert_eq!(output.matches("(y/n/d)").count(), 2, "{output}");
}

#[test]
fn an_unknown_answer_asks_again() {
    let (choice, output) = ask("maybe\nn\n");
    assert_eq!(choice, Choice::Keep);
    assert!(output.contains("answer y to overwrite"), "{output}");
}

#[test]
fn no_answer_keeps_the_copy() {
    assert_eq!(ask("").0, Choice::Keep);
}
