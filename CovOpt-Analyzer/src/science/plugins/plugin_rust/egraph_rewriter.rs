//! E-Graph (egg) rewrites for mathematical and logical collapse.
//!
//! Uses equality graphs to collapse redundant AST structures (e.g., `x * 1 => x`) 
//! before they are evaluated by the Crucible, preventing combinatorial explosion.

use egg::{rewrite as rw, *};

define_language! {
    pub enum MathLang {
        Num(i32),
        "+" = Add([Id; 2]),
        "*" = Mul([Id; 2]),
        "-" = Sub([Id; 2]),
        "/" = Div([Id; 2]),
        "sin" = Sin(Id),
        "cos" = Cos(Id),
        Symbol(Symbol),
    }
}

pub struct EGraphRewriter;

impl EGraphRewriter {
    pub fn optimize_math(expression: &str) -> String {
        let rules: &[Rewrite<MathLang, ()>] = &[
            rw!("add-zero"; "(+ ?a 0)" => "?a"),
            rw!("mul-one";  "(* ?a 1)" => "?a"),
            rw!("mul-zero"; "(* ?a 0)" => "0"),
            rw!("sub-self"; "(- ?a ?a)" => "0"),
            rw!("div-self"; "(/ ?a ?a)" => "1" if is_not_zero("?a")),
        ];

        let expr: RecExpr<MathLang> = expression.parse().unwrap();
        let mut runner = Runner::default().with_expr(&expr).run(rules);
        let root = runner.roots[0];
        
        let extractor = Extractor::new(&runner.egraph, AstSize);
        let (_, best_expr) = extractor.find_best(root);
        best_expr.to_string()
    }
}

fn is_not_zero(var: &'static str) -> impl Fn(&mut EGraph<MathLang, ()>, Id, &Subst) -> bool {
    let var = var.parse().unwrap();
    move |egraph, _, subst| {
        if let Some(MathLang::Num(0)) = egraph[subst[var]].nodes.first() {
            false
        } else {
            true
        }
    }
}
