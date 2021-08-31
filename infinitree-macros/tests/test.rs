use infinitree::index::ChunkIndex;
use infinitree_macros::Index;

#[derive(Default, Index)]
pub struct TestStructNeedsToCompile {
    /// A field with both an accessor method and serialized to storage
    chunks: ChunkIndex,
}

#[derive(Default, Index)]
pub struct TestStructWithAttributes {
    /// Rename the field to `renamed_chunks` both in serialized form
    /// and accessor method
    #[infinitree(name = "renamed_chunks")]
    chunks: ChunkIndex,

    /// Skip generating accessors and exclude from on-disk structure
    #[infinitree(skip)]
    _unreferenced: ChunkIndex,

    /// Skip generating accessors and exclude from on-disk structure
    #[infinitree(strategy = "infinitree::index::SparseField")]
    strategizing: ChunkIndex,
}

#[test]
fn the_code_is_useless_but_must_compile() {
    let mut s = TestStructNeedsToCompile::default();
    let _ = s.chunks();

    let mut with_attributes = TestStructWithAttributes::default();
    let _ = with_attributes.renamed_chunks();
    let _ = with_attributes.strategizing();

    // uncommenting this will fail the build
    // let _ = with_attributes.unreferenced();
}
