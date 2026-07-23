use syn::visit::Visit;

struct DeepestLoopVisitor {
    current_depth: usize,
    max_depth: usize,
    deepest_line: Option<usize>,
}

impl<'ast> Visit<'ast> for DeepestLoopVisitor {
    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.max_depth = self.current_depth;
            self.deepest_line = Some(node.for_token.span.start().line);
        }
        syn::visit::visit_expr_for_loop(self, node);
        self.current_depth -= 1;
    }
    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.max_depth = self.current_depth;
            self.deepest_line = Some(node.while_token.span.start().line);
        }
        syn::visit::visit_expr_while(self, node);
        self.current_depth -= 1;
    }
    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.max_depth = self.current_depth;
            self.deepest_line = Some(node.loop_token.span.start().line);
        }
        syn::visit::visit_expr_loop(self, node);
        self.current_depth -= 1;
    }
}

fn main() {
    let src = r#"
        fn process_data() {
            let mut x = 0;
            for i in 0..10 {
                while x < 5 {
                    loop {
                        x += 1;
                        break;
                    }
                }
            }
        }
    "#;

    let ast: syn::File = syn::parse_str(src).unwrap();
    let mut visitor = DeepestLoopVisitor { current_depth: 0, max_depth: 0, deepest_line: None };
    visitor.visit_file(&ast);

    println!("Deepest line: {:?}", visitor.deepest_line);
}
