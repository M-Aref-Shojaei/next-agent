- {{query (and [[Dune]] (task TODO DOING))}}
- #+BEGIN_QUERY
  {:title "Advanced"
   :query [:find (pull ?b [*]) :where [?b :block/marker "TODO"]]}
  #+END_QUERY
