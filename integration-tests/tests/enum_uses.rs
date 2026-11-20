#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::enum_uses::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that each use of the same literals reads its own enum.
#[allow(dead_code)]
fn each_use_has_its_enum(reasoning: &Reasoning, pen: &Pen) {
    let _: Option<ReasoningGenerateSummary> = reasoning.generate_summary();
    let _: Option<ReasoningSummary> = reasoning.summary();
    reasoning.set_summary(ReasoningSummary::Auto);
    reasoning.set_generate_summary(ReasoningGenerateSummary::Concise);
    // A parameter of the property's name shares the property's enum.
    reasoning.retry(ReasoningSummary::Concise);
    let _: ReasoningSwapReturn = reasoning.swap(ReasoningMode::Fast);

    let _: PenTip = Pen::tip();
    Pen::set_cap(PenCap::Fine);
    let _: PenInk = pen.ink();
}
