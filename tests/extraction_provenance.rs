use egglog::extract::{
    DEFAULT_COST_MODEL, ExtractionDag, ExtractionOrigin, OriginId, TreeExtractor,
};
use egglog::{EGraph, Term};

fn eval(egraph: &mut EGraph, source: &str) -> (egglog::ArcSort, egglog::Value) {
    let expression = egraph.parser.get_expr_from_string(None, source).unwrap();
    egraph.eval_expr(&expression).unwrap()
}

fn children(dag: &ExtractionDag, origin: OriginId) -> &[OriginId] {
    match dag.origins().get(origin) {
        ExtractionOrigin::Apply { children, .. } => children,
        origin => panic!("expected application, got {origin:?}"),
    }
}

fn spelling(dag: &ExtractionDag, origin: OriginId) -> OriginId {
    match dag.origins().get(origin) {
        ExtractionOrigin::Value { spelling, .. } => *spelling,
        origin => panic!("expected typed value, got {origin:?}"),
    }
}

// Opaque typed annotations deliberately distinguish equal test spellings.
// Normalizers must transport these annotations, not inspect or recreate them.
fn tagged_spelling(
    egraph: &mut EGraph,
    dag: &mut ExtractionDag,
    term: egglog::TermId,
    tag: &str,
) -> OriginId {
    let (sort, value) = eval(egraph, tag);
    let spelling = dag.spelled(term);
    dag.value(sort, value, spelling)
}

fn assert_topological(dag: &ExtractionDag) {
    assert!(dag.origins().iter().all(|(id, origin)| match origin {
        ExtractionOrigin::Spelled(term) => *term == id.term(),
        ExtractionOrigin::Apply { term, children } => {
            *term == id.term() && children.iter().all(|child| child.index() < id.index())
        }
        ExtractionOrigin::Value { spelling, .. } => {
            spelling.term() == id.term() && spelling.index() < id.index()
        }
    }));
}

#[test]
fn set_provenance_follows_native_ast_order_and_coalescing() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(None, "(sort IntSet (Set i64))")
        .unwrap();
    let (sort, value) = eval(&mut egraph, "(set-of)");
    let mut dag = ExtractionDag::default();
    let terms = [2, 10, 1].map(|number| dag.termdag_mut().lit(egglog::ast::Literal::Int(number)));
    let origins = [(terms[0], "2"), (terms[1], "10"), (terms[2], "1")]
        .map(|(term, tag)| tagged_spelling(&mut egraph, &mut dag, term, tag));
    let alias = tagged_spelling(&mut egraph, &mut dag, terms[0], "3");
    assert_ne!(origins[0], alias);
    let root = egraph.reconstruct_container_value(
        &sort,
        value,
        &mut dag,
        vec![origins[0], origins[1], origins[2], alias],
    );
    assert_eq!(dag.termdag().to_string(root.term()), "(set-of 1 2 10)");
    // BTreeSet::from_iter retains the last equal carrier, just as the native
    // term-only normalizer's collection does; no matching pass chooses it.
    assert_eq!(children(&dag, root), &[origins[2], alias, origins[1]]);
    assert_topological(&dag);
}

#[test]
fn map_provenance_retains_native_duplicate_key_and_value_selections() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(None, "(sort IntMap (Map i64 i64))")
        .unwrap();
    let (sort, value) = eval(&mut egraph, "(map-of)");
    let mut dag = ExtractionDag::default();
    let key_term = dag.termdag_mut().lit(egglog::ast::Literal::Int(2));
    let first_key = tagged_spelling(&mut egraph, &mut dag, key_term, "2");
    let last_key = tagged_spelling(&mut egraph, &mut dag, key_term, "3");
    assert_ne!(first_key, last_key);
    let first_value_term = dag.termdag_mut().lit(egglog::ast::Literal::Int(30));
    let first_value = dag.spelled(first_value_term);
    let last_value_term = dag.termdag_mut().lit(egglog::ast::Literal::Int(40));
    let last_value = dag.spelled(last_value_term);
    let root = egraph.reconstruct_container_value(
        &sort,
        value,
        &mut dag,
        vec![first_key, first_value, last_key, last_value],
    );
    assert_eq!(dag.termdag().to_string(root.term()), "(map-of 2 40)");
    // The owner uses BTreeMap::from_iter, not sequential insert: its equal-key
    // carrier and value both come from the final pair.
    assert_eq!(children(&dag, root), &[last_key, last_value]);
    assert_topological(&dag);
}

#[test]
fn multiset_provenance_preserves_stable_repeated_occurrences() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(None, "(sort IntBag (MultiSet i64))")
        .unwrap();
    let (sort, value) = eval(&mut egraph, "(multiset-of)");
    let mut dag = ExtractionDag::default();
    let repeated_term = dag.termdag_mut().lit(egglog::ast::Literal::Int(2));
    let repeated =
        ["2", "3", "4"].map(|tag| tagged_spelling(&mut egraph, &mut dag, repeated_term, tag));
    assert_ne!(repeated[0], repeated[1]);
    assert_ne!(repeated[1], repeated[2]);
    let first_term = dag.termdag_mut().lit(egglog::ast::Literal::Int(10));
    let first = dag.spelled(first_term);
    let root = egraph.reconstruct_container_value(
        &sort,
        value,
        &mut dag,
        vec![repeated[0], first, repeated[1], repeated[2]],
    );
    assert_eq!(
        dag.termdag().to_string(root.term()),
        "(multiset-of 2 2 2 10)"
    );
    assert_eq!(
        children(&dag, root),
        &[repeated[0], repeated[1], repeated[2], first]
    );
    assert_topological(&dag);
}

#[test]
fn vector_provenance_preserves_empty_constructor_and_shared_polymorphic_spelling() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(
            None,
            "(sort IntVec (Vec i64)) (sort StringVec (Vec String))",
        )
        .unwrap();
    let roots = [
        "(vec-pop (vec-of 1))",
        "(vec-pop (vec-of \"x\"))",
        "(vec-of 3 1)",
    ]
    .map(|source| eval(&mut egraph, source));
    let extracted = egraph.extract_best(roots.into()).unwrap();
    let roots = extracted
        .terms
        .iter()
        .map(|root| root.as_ref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(roots[0].term(), roots[1].term());
    assert_ne!(roots[0].origin, roots[1].origin);
    assert_eq!(
        extracted.dag.termdag().to_string(roots[0].term()),
        "(vec-empty)"
    );
    assert_eq!(
        extracted.dag.termdag().to_string(roots[2].term()),
        "(vec-of 3 1)"
    );
    assert!(children(&extracted.dag, spelling(&extracted.dag, roots[0].origin)).is_empty());
    assert!(children(&extracted.dag, spelling(&extracted.dag, roots[1].origin)).is_empty());
    assert_eq!(
        children(&extracted.dag, spelling(&extracted.dag, roots[2].origin)).len(),
        2
    );
    assert_topological(&extracted.dag);
}

#[test]
fn tree_provenance_shares_selected_children_and_batch_roots() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(None, "(datatype Tree (Leaf i64) (Pair Tree Tree))")
        .unwrap();
    let root = eval(&mut egraph, "(Pair (Leaf 7) (Leaf 7))");
    let extracted = egraph.extract_best(vec![root.clone(), root]).unwrap();
    let first = extracted.terms[0].as_ref().unwrap();
    let second = extracted.terms[1].as_ref().unwrap();
    assert_eq!(first.origin, second.origin);
    assert_eq!(first.cost, 5);
    assert_eq!(
        extracted.dag.termdag().to_string(first.term()),
        "(Pair (Leaf 7) (Leaf 7))"
    );
    let selected = children(&extracted.dag, spelling(&extracted.dag, first.origin));
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0], selected[1]);
    assert_eq!(extracted.dag.origins().len(), 6);
    assert_topological(&extracted.dag);
}

#[test]
fn prepared_tree_repeated_reads_share_exact_origins_without_a_selection_cache() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(None, "(datatype Tree (Leaf i64) (Pair Tree Tree))")
        .unwrap();
    let (sort, value) = eval(&mut egraph, "(Pair (Leaf 7) (Leaf 7))");
    let extractor = TreeExtractor::compute_costs_from_rootsorts(
        Some(vec![sort.clone()]),
        &egraph,
        DEFAULT_COST_MODEL,
    );
    let mut dag = ExtractionDag::default();
    let roots = (0..64)
        .map(|_| {
            extractor
                .extract_best_with_sort(&mut dag, value, sort.clone())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        roots
            .iter()
            .all(|root| root.origin == roots[0].origin && root.cost == 5)
    );
    assert_eq!(dag.origins().len(), 6);
    assert_eq!(dag.termdag().size(), 3);
    assert_topological(&dag);
}

#[test]
fn tree_variants_retain_distinct_root_selections_and_shared_child_origins() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(
            None,
            "(datatype Tree (Leaf i64) (Other i64)) (union (Leaf 7) (Other 7))",
        )
        .unwrap();
    let root = eval(&mut egraph, "(Leaf 7)");
    let extracted = egraph.extract_variants(vec![root], 2).unwrap();
    let variants = &extracted.variants[0];
    assert_eq!(variants.len(), 2);
    assert_ne!(variants[0].origin, variants[1].origin);
    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.cost)
            .collect::<Vec<_>>(),
        [2, 2]
    );
    let selections = variants
        .iter()
        .map(|variant| children(&extracted.dag, spelling(&extracted.dag, variant.origin)))
        .collect::<Vec<_>>();
    assert_eq!(selections[0], selections[1]);
    assert_eq!(extracted.dag.origins().len(), 6);
    assert_topological(&extracted.dag);
}

#[test]
fn function_provenance_separates_synthetic_target_from_typed_captures() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(
            None,
            "(datatype Tree (Leaf i64) (Pair Tree Tree)) (sort PairFn (UnstableFn (Tree) Tree))",
        )
        .unwrap();
    let root = eval(&mut egraph, "(unstable-fn \"Pair\" (Leaf 7))");
    let extracted = egraph.extract_best(vec![root]).unwrap();
    let root = extracted.terms[0].as_ref().unwrap();
    let selected = children(&extracted.dag, spelling(&extracted.dag, root.origin));
    assert_eq!(selected.len(), 2);
    assert!(matches!(
        extracted.dag.origins().get(selected[0]),
        ExtractionOrigin::Spelled(_)
    ));
    assert!(matches!(
        extracted.dag.termdag().get(selected[0].term()),
        Term::Lit(_)
    ));
    assert!(
        matches!(extracted.dag.origins().get(selected[1]), ExtractionOrigin::Value { sort, .. } if sort.name() == "Tree")
    );
    assert_eq!(
        extracted.dag.termdag().to_string(root.term()),
        "(unstable-fn \"Pair\" (Leaf 7))"
    );
    assert_topological(&extracted.dag);
}
