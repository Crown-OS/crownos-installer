//! Either flow: the account the user will sign in with.
//!
//! The payload deliberately carries no password — it sits on a partition any OS
//! on the machine can read — which is why this page cannot be skipped even for
//! an unattended install.

use crownui::kit::{ButtonVariant, TextInputOptions, text_input};
use crownui::prelude::{Cx, Listeners, Semantic, Signal, Styled, View, bind, keyed, vstack};

use crate::ui::components::{Tone, alert, field};
use crate::ui::layout::{BLOCK_GAP, PageParts, back_pill, nothing, wizard_pill};
use crate::ui::{Form, Wizard};
use crate::util::mirror::mirror;

const FIELD_GAP_X: f32 = 24.0;
const FIELD_GAP_Y: f32 = 20.0;

/// Which [`Form`] field an input edits, and whether the plan depends on it.
pub struct FormField {
    pub name: &'static str,
    placeholder: &'static str,
    read: fn(&Form) -> &String,
    write: fn(&mut Form) -> &mut String,
    replans: bool,
    secret: bool,
}

pub const FIELDS: &[FormField] = &[
    FormField {
        name: "Full name",
        placeholder: "Optional",
        read: |form| &form.full_name,
        write: |form| &mut form.full_name,
        replans: false,
        secret: false,
    },
    FormField {
        name: "Username",
        placeholder: "crown",
        read: |form| &form.username,
        write: |form| &mut form.username,
        replans: true,
        secret: false,
    },
    FormField {
        name: "Password",
        placeholder: "",
        read: |form| &form.password,
        write: |form| &mut form.password,
        replans: false,
        secret: true,
    },
    FormField {
        name: "Repeat password",
        placeholder: "",
        read: |form| &form.confirm,
        write: |form| &mut form.confirm,
        replans: false,
        secret: true,
    },
    FormField {
        name: "Computer name",
        placeholder: "crownos",
        read: |form| &form.hostname,
        write: |form| &mut form.hostname,
        replans: true,
        secret: false,
    },
];

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let auto = wizard.with_untracked(cx, |wizard| {
        wizard.context().is_some_and(|context| context.auto_install)
    });
    let subtitle = if auto {
        "Only the password is asked here; the rest came from Windows."
    } else {
        "Create your account"
    };

    let fields: Vec<_> = FIELDS
        .iter()
        .map(|form_field| input(cx, wizard, form_field))
        .collect();

    // Quiet until the user has tried to continue, so the page does not shout
    // about an empty password before they have had a chance to type one.
    let problem = keyed(
        move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard.form.touched.then(|| wizard.form.problem()).flatten()
            })
        },
        |_, problem| match problem {
            Some(problem) => alert(Tone::Danger, "Not yet", problem).boxed(),
            None => nothing(),
        },
    );

    let form = vstack(fields)
        .grid()
        .grid_cols(2)
        .gap_x(FIELD_GAP_X)
        .gap_y(FIELD_GAP_Y)
        .w_full();

    PageParts::new("Let's get you started", subtitle)
        .content(vstack((form, problem)).gap(BLOCK_GAP))
        .actions((
            back_pill(wizard),
            wizard_pill(
                wizard,
                "Next",
                ButtonVariant::Primary,
                Wizard::submit_account,
            )
            .disabled(bind(move |runtime| {
                wizard.with(runtime, |wizard| wizard.form.problem().is_some())
            })),
        ))
}

fn input(cx: &mut Cx, wizard: Signal<Wizard>, form_field: &'static FormField) -> impl View + use<> {
    let value = mirror(
        cx,
        wizard,
        |wizard| (form_field.read)(&wizard.form).clone(),
        |wizard, value| {
            *(form_field.write)(&mut wizard.form) = value;
            if form_field.replans {
                wizard.restage_plan();
            }
        },
    );
    let entry = text_input(value)
        .placeholder(form_field.placeholder)
        .on_submit(move |cx, _| wizard.update(cx, Wizard::submit_account));
    let entry = if form_field.secret {
        entry.secret()
    } else {
        entry
    };
    field(form_field.name, entry)
}
