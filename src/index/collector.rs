use tantivy::collector::{Collector, SegmentCollector};
use tantivy::{DocAddress, DocId, Score, SegmentOrdinal, SegmentReader};

pub struct CollectAll;

pub struct CollectSegment {
    fruit: Vec<(Score, DocAddress)>,
    segment: SegmentOrdinal,
}

impl Collector for CollectAll {
    type Fruit = Vec<(Score, DocAddress)>;
    type Child = CollectSegment;

    fn for_segment(
        &self,
        segment: SegmentOrdinal,
        _: &SegmentReader,
    ) -> tantivy::Result<Self::Child> {
        Ok(CollectSegment {
            fruit: Vec::new(),
            segment,
        })
    }

    fn requires_scoring(&self) -> bool {
        false
    }

    fn merge_fruits(
        &self,
        segment_fruits: Vec<<Self::Child as SegmentCollector>::Fruit>,
    ) -> tantivy::Result<Self::Fruit> {
        Ok(segment_fruits.concat())
    }
}

impl SegmentCollector for CollectSegment {
    type Fruit = Vec<(Score, DocAddress)>;

    fn collect(&mut self, doc_id: DocId, _: Score) {
        self.fruit.push((
            Default::default(),
            DocAddress {
                doc_id,
                segment_ord: self.segment,
            },
        ));
    }

    fn harvest(self) -> Self::Fruit {
        self.fruit
    }
}
