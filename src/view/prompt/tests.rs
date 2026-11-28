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

fn confirmed(answers: &str) -> (bool, String) {
    let mut output = Vec::new();
    let yes = confirm(REBUILD, &mut answers.as_bytes(), &mut output).unwrap();
    (yes, String::from_utf8(output).unwrap())
}

#[test]
fn confirm_takes_y_or_n() {
    assert!(confirmed("y\n").0);
    assert!(confirmed("Yes\n").0);
    assert!(!confirmed("n\n").0);
}

#[test]
fn confirm_asks_again_then_the_end_of_input_is_no() {
    let (yes, output) = confirmed("maybe\n");
    assert!(!yes);
    assert_eq!(output.matches(REBUILD).count(), 2, "{output}");
    assert!(output.starts_with(
        "Prior values will be kept, formatting and comments will be lost. Continue... (y/n) "
    ));
}
