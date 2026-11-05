//! How deep `usvg` and `resvg` recurse into a drawing, found before they run.
//!
//! Their recursion follows more than the XML: a `<pattern>` filled by another pattern, a clip path
//! clipped by another, a `<use>` of a group holding a `<use>` all nest one level per link, and such
//! a chain can be written flat, one line per link (5 000 patterns are 500 KB of XML and need 64 MB
//! of stack). So the depth here is the longest path through the XML tree and the references
//! together, wherever those references are written: an attribute, a `style` attribute or a rule in
//! a `<style>` sheet.

use std::collections::HashMap;

use resvg::usvg::roxmltree::{Document, Node, NodeId};

/// An element as `simplecss` sees it, matched the way `usvg` matches its own style sheets.
struct Styled<'a, 'input>(Node<'a, 'input>);

impl simplecss::Element for Styled<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(Styled)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(Styled)
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }

    fn attribute_matches(&self, name: &str, operator: simplecss::AttributeOperator) -> bool {
        self.0.attribute(name).is_some_and(|value| operator.matches(value))
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild) && self.0.prev_sibling_element().is_none()
    }
}

/// Every id named by a `url(#id)` in `value`, quoted or not.
fn url_targets(value: &str, found: &mut impl FnMut(&str)) {
    let mut rest = value;
    while let Some(at) = rest.find("url(") {
        rest = rest.get(at.saturating_add(4)..).unwrap_or_default();
        if let Some(id) = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '"' || c == '\'').strip_prefix('#') {
            let end = id.find(|c: char| c.is_ascii_whitespace() || matches!(c, ')' | '"' | '\'')).unwrap_or(id.len());
            found(id.get(..end).unwrap_or_default());
        }
    }
}

/// The ids `element` refers to through its own attributes and the rules that match it.
fn references(element: Node, rules: &[&simplecss::Rule], found: &mut impl FnMut(&str)) {
    for attribute in element.attributes() {
        url_targets(attribute.value(), found);
        if attribute.name() == "href"
            && let Some(id) = attribute.value().trim().strip_prefix('#')
        {
            found(id);
        }
    }
    for rule in rules.iter().filter(|rule| rule.selector.matches(&Styled(element))) {
        for declaration in &rule.declarations {
            url_targets(declaration.value, found);
        }
    }
}

/// The longest chain of elements `usvg` and `resvg` may recurse along, counted as
/// [`nesting_depth`](super::nesting_depth) counts: a leaf adds nothing. Iterative, so `doc` can be
/// any shape: elements are peeled off once everything they reach has its height. What is left
/// reaches a reference cycle, which `usvg` breaks without saying where, so each of those elements
/// counts once: no chain can go through more of them than there are.
pub(super) fn drawing_depth(doc: &Document) -> usize {
    let mut sheet = simplecss::StyleSheet::new();
    for style in doc.descendants().filter(|node| node.has_tag_name("style")) {
        if matches!(style.attribute("type"), Some("text/css") | None)
            && let Some(text) = style.text()
        {
            sheet.parse_more(text);
        }
    }
    let rules: Vec<_> = sheet.rules.iter().filter(|rule| rule.declarations.iter().any(|declaration| declaration.value.contains("url("))).collect();

    // The first element with an id wins, as in `usvg`.
    let mut ids: HashMap<&str, NodeId> = HashMap::new();
    for node in doc.descendants() {
        if let Some(id) = node.attribute("id") {
            ids.entry(id).or_insert_with(|| node.id());
        }
    }

    let slots = doc.descendants().map(|node| node.id().get_usize().saturating_add(1)).max().unwrap_or(0);
    // Per element: what it reaches that has no height yet, and its height so far.
    let (mut waiting, mut height) = (vec![0u32; slots], vec![0u32; slots]);
    let mut referrers: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut elements = 0usize;
    for element in doc.descendants().filter(Node::is_element) {
        elements = elements.saturating_add(1);
        if let Some(slot) = element.parent_element().and_then(|parent| waiting.get_mut(parent.id().get_usize())) {
            *slot = slot.saturating_add(1);
        }
        references(element, &rules, &mut |id| {
            if let Some(target) = ids.get(id) {
                referrers.entry(*target).or_default().push(element.id());
                if let Some(slot) = waiting.get_mut(element.id().get_usize()) {
                    *slot = slot.saturating_add(1);
                }
            }
        });
    }

    let mut ready: Vec<NodeId> =
        doc.descendants().filter(|node| node.is_element() && waiting.get(node.id().get_usize()) == Some(&0)).map(|node| node.id()).collect();
    let (mut deepest, mut peeled) = (0u32, 0usize);
    while let Some(id) = ready.pop() {
        peeled = peeled.saturating_add(1);
        let above = height.get(id.get_usize()).copied().unwrap_or(0);
        deepest = deepest.max(above);
        let parent = doc.get_node(id).and_then(|node| node.parent_element()).map(|parent| parent.id());
        for caller in parent.iter().chain(referrers.get(&id).into_iter().flatten()) {
            if let Some(slot) = height.get_mut(caller.get_usize()) {
                *slot = (*slot).max(above.saturating_add(1));
            }
            if let Some(slot) = waiting.get_mut(caller.get_usize()) {
                *slot = slot.saturating_sub(1);
                if *slot == 0 {
                    ready.push(*caller);
                }
            }
        }
    }
    usize::try_from(deepest).unwrap_or(usize::MAX).saturating_add(elements.saturating_sub(peeled))
}

#[cfg(test)]
mod tests {
    use resvg::usvg::roxmltree::{Document, ParsingOptions};

    fn depth(xml: &str) -> usize {
        let doc = Document::parse_with_options(xml, ParsingOptions { allow_dtd: true, ..Default::default() }).unwrap();
        super::drawing_depth(&doc)
    }

    #[test]
    fn plain_nesting_counts_like_the_markup_check() {
        for xml in ["<svg/>", "<svg><rect/></svg>", "<svg><g><g><rect/></g></g></svg>", "<svg><g/><g><!-- <g> --><rect/></g><g/></svg>"] {
            assert_eq!(depth(xml), super::super::nesting_depth(xml.as_bytes(), 99), "{xml}");
        }
    }

    #[test]
    fn a_reference_adds_what_its_target_reaches() {
        let target = "<g id=\"t\"><g><g><rect/></g></g></g>";
        // <svg> → <rect> → the target's three groups.
        for reference in ["fill=\"url(#t)\"", "fill='url( \"#t\" )'", "style=\"fill:url(#t)\"", "href=\"#t\"", "href=\" #t \"", "filter=\"url(#x) url(#t)\""] {
            assert_eq!(depth(&format!("<svg>{target}<rect {reference}/></svg>")), 5, "{reference}");
        }
        assert_eq!(depth(&format!("<svg xmlns:x=\"http://www.w3.org/1999/xlink\">{target}<rect x:href=\"#t\"/></svg>")), 5);
    }

    #[test]
    fn a_style_sheet_reference_reaches_only_the_elements_it_matches() {
        let drawing = |rule: &str| format!("<svg><style>{rule}</style><g id=\"t\"><g><g><rect/></g></g></g><rect class=\"c\"/></svg>");
        assert_eq!(depth(&drawing(".c{fill:url(#t)}")), 5);
        assert_eq!(depth(&drawing(".other{fill:url(#t)}")), 4);
        assert_eq!(depth(&drawing(".c{fill:#000}")), 4);
    }

    #[test]
    fn references_that_lead_nowhere_add_nothing() {
        assert_eq!(depth("<svg><rect fill=\"url(#missing)\" href=\"https://example.com/#a\"/><rect fill=\"url(\"/></svg>"), 1);
    }

    /// Everything that reaches a cycle counts, so a cycle can't hide what lies behind it.
    #[test]
    fn a_cycle_counts_every_element_that_reaches_it() {
        assert_eq!(depth("<svg><g id=\"a\"><rect fill=\"url(#a)\"/></g></svg>"), 3);
        assert_eq!(depth("<svg><rect/></svg>"), 1);
    }

    #[test]
    fn a_first_id_wins_as_in_usvg() {
        assert_eq!(depth("<svg><rect id=\"t\"/><g id=\"t\"><g><rect/></g></g><rect fill=\"url(#t)\"/></svg>"), 3);
    }

    #[test]
    fn entities_are_measured_expanded() {
        let xml = "<!DOCTYPE svg [<!ENTITY deep \"<g><g><g><rect/></g></g></g>\">]><svg><g>&deep;</g></svg>";
        assert_eq!(depth(xml), 5);
    }
}
