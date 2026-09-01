//! The pin for `common::tracked`: an untracked file is not in the corpus.
//!
//! Proven once by hand with an untracked rule the plugin guard could not see
//! and the same rule, tracked, that failed it — and a defect worth fixing is
//! worth holding in place, because the walk it replaced is the natural thing
//! to write back.

mod common;

#[test]
fn an_untracked_file_is_not_part_of_the_corpus() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    let run = |args: &[&str]| {
        let status = common::git(root).args(args).status().unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/tracked.md"), "tracked\n").unwrap();
    std::fs::write(root.join("docs/scratch.md"), "untracked\n").unwrap();
    run(&["add", "docs/tracked.md"]);

    let corpus = common::tracked(root, "docs");
    assert_eq!(corpus, vec![root.join("docs/tracked.md")]);

    // The positive control: track the same file and it is read.
    run(&["add", "docs/scratch.md"]);
    let mut corpus = common::tracked(root, "docs");
    corpus.sort();
    assert_eq!(
        corpus,
        vec![root.join("docs/scratch.md"), root.join("docs/tracked.md")]
    );
}
