use super::*;
use pretty_assertions::assert_eq;

#[test]
fn converts_a_display_block_to_unicode_rows() {
    let source = concat!(
        "The project's confidence score is modeled as:\n",
        "\n",
        "\\[\n",
        "\\text{Confidence} =\n",
        "\\frac{\\text{Evidence} \\times \\text{Repetition}}{\\text{Uncertainty} + 1}\n",
        "\\]\n",
        "\n",
        "Done.\n",
    );

    assert_eq!(
        rewrite_math(source).as_ref(),
        concat!(
            "The project's confidence score is modeled as:\n",
            "\n",
            "Confidence =  \n",
            "(Evidence × Repetition) / (Uncertainty + 1)\n",
            "\n",
            "Done.\n",
        )
    );
}

#[test]
fn converts_dollar_display_blocks() {
    assert_eq!(rewrite_math("$$\nE = mc^2\n$$\n").as_ref(), "E = mc²\n");
}

#[test]
fn converts_inline_math_and_leaves_code_alone() {
    assert_eq!(
        rewrite_math("Let \\(x_1 \\le y\\) hold, but `\\(x\\)` stays.\n").as_ref(),
        "Let x₁ ≤ y hold, but `\\(x\\)` stays.\n"
    );
}

#[test]
fn leaves_unclosed_and_fenced_math_untouched() {
    let streaming_prefix = "\\[\n\\text{Confidence} =\n";
    assert_eq!(rewrite_math(streaming_prefix).as_ref(), streaming_prefix);

    let fenced = "```latex\n\\[\nx = 1\n\\]\n```\n";
    assert_eq!(rewrite_math(fenced).as_ref(), fenced);
}

#[test]
fn converts_common_constructs() {
    assert_eq!(latex_to_unicode("\\sqrt{x + 1}"), "√(x + 1)");
    assert_eq!(latex_to_unicode("\\sum_{i=1}^{n} a_i"), "∑_(i=1)ⁿ aᵢ");
    assert_eq!(latex_to_unicode("\\alpha \\to \\beta"), "α → β");
    assert_eq!(latex_to_unicode("\\frac{1}{2}"), "1 / 2");
}

#[test]
fn aligned_equations_are_readable() {
    let source = concat!(
        "$$\n",
        "\\begin{aligned}\n",
        "\\nabla \\cdot \\mathbf{E} &= \\frac{\\rho}{\\varepsilon_0} \\\\\n",
        "\\nabla \\cdot \\mathbf{B} &= 0 \\\\\n",
        "\\nabla \\times \\mathbf{E} &= -\\frac{\\partial \\mathbf{B}}{\\partial t} \\\\\n",
        "\\nabla \\times \\mathbf{B} &= \\mu_0\\mathbf{J}\n",
        "\\end{aligned}\n",
        "$$\n",
    );
    assert_eq!(
        rewrite_math(source).as_ref(),
        concat!(
            "∇ · E = ρ / ε₀  \n",
            "∇ · B = 0  \n",
            "∇ × E = -(∂ B) / (∂ t)  \n",
            "∇ × B = μ₀J\n",
        )
    );
}
