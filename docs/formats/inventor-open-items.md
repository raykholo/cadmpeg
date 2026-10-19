# Autodesk Inventor IPT/IAM: Open Items

This document lists unknown Inventor byte semantics. [inventor.md](inventor.md) contains the settled format model.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. RSe selection and metadata

### RS-01. Multiple coherent databases

**Question.** Which typed field selects one database when several `V<n>/RSeDb` streams have schema 31 but differ in identity or save state?

**Known.** The storage-band number and database schema are independent. The decoder parses all database candidates and requires one coherent registry grammar.

**Need.** We must select one database without using path order or a save-version string.

**Note.** `coherent_schema` ignores unavailable database candidates and takes the first parsed schema before checking the remaining parsed schemas. A malformed candidate can therefore be absent from the grammar decision; database identity and save state remain unselected.

### RS-02. Metadata sections 5 and 6

**Question.** What item grammar and discriminator semantics apply to metadata sections 5 and 6?

**Known.** The backward spans establish their exact ranges and their join to section 4.

**Need.** We must type their records and references.

### RS-03. Section 7 long item form

**Question.** What fields select and define the section 7 item form whose item width is at least 76 bytes?

**Known.** The short section 7 form uses 32-byte items. Both forms have exact backward-framed ranges.

**Need.** We must validate the long item sequence without accepting arbitrary payload widths.

### RS-04. Revision and record-trailer variants

**Question.** Which revision-table versions, extended record-trailer property types, and trailer list forms govern RSe database variants outside the supported branch?

**Known.** The current envelope parses revision-table version 3 and the implemented trailer property and reference-list forms. Unsupported revision versions and trailer property types are refused after their owning record boundary is established.

**Need.** Each revision and trailer variant requires an exact field grammar, range rule, and native retention policy before semantic decode.

**Note.** `parse_revisions` treats every nonzero `kind == u16::MAX` payload selector as the short form and zero as the long form. It does not establish whether other selector values are valid or whether the selector has this polarity.

### RS-05. Registry object and node-count ownership

**Question.** Which registry object owns the count of node records that follow an entry, and what consistency rule applies when an entry contains more than one object?

**Known.** Each registry object stores a `node_count`. The current grammar uses the last object's value minus one as the entry's following node-record count. It does not require the object values to agree or identify the count-owning object.

**Need.** The count owner, bias, and multi-object consistency rule must be exact before the value can frame the following node records.

### RS-06. Record selector high bits

**Question.** What do bits 8 through 31 of a bulk-record selector encode, and how does a record select a type descriptor at index 256 or later?

**Known.** The selector low byte indexes the section 4 type table. Bits 8 through 15 hold 1 and bits 16 through 31 hold 0 in every record. A section 4 table can hold more than 256 descriptors; the descriptors after index 255 have no record that selects them.

**Need.** The selector width must be exact before a segment whose records use more than 256 record types can be framed.

## 2. Kernel ownership

### KE-01. Multiple part carriers

**Question.** Which typed ownership and history references select the active model when a part contains more than one `PmBRep` segment or kernel-carrier record?

**Known.** The supported envelope has one `PmBRep` segment and one typed kernel-carrier record. Carrier signatures do not establish ownership.

**Need.** We must select active and historical carriers without appending all geometry.

### KE-02. ACIS save-format and carrier-footer bands

**Question.** Which ACIS save-format bands outside 217/218 and which segment-version carrier footers use a compatible SAB grammar and active-carrier semantic contract?

**Known.** ACIS 217 and 218 are the verified bands of the 32-bit header and SAB decoder. Other ACIS save-format bands take the same framing and record decode, carry an `Admission::Unverified` kernel layer with a `source.dialect-unverified` charge, and retain the carrier bytes when they read no geometry. The current carrier footer branch supports segment major versions 15 through 22 and 23 and later; earlier or otherwise incompatible variants are refused.

**Need.** Each additional ACIS or footer band requires direct-carrier framing, geometry, validation, and wrapper-parity evidence before activation.

**Note.** The original `KE-02` was removed by commit `87e40474c` after the 217/218 branch was added. That commit contains a shared implementation and synthetic 217/218 fixtures, but it does not establish the grammar for other ACIS bands or the remaining carrier-footer variants. The item is reopened here with the narrower scope above.

## 3. Product structure

### AS-01. Local prototypes and parent records

**Question.** Which record families define embedded local prototypes and parent-child placement inside one document?

**Known.** A `UFRxDoc` occurrence joins its external reference through its first-section document identifier, narrowed by component name when copies share that identifier, and joins `AmDc` and `AmGraphics` through the exact occurrence identifier. Current-document external occurrences are root placements. A referenced assembly remains one unresolved external prototype because the codec does not load it. External-reference state bit `0x2000` marks a suppressed occurrence.

The schema-15 representation/model-state branch and its occurrence table are framed exactly. Unsupported property and export tags stop semantic transfer without inventing a record boundary.

**Need.** We must establish local component definitions and parent links from typed records before emitting non-root or local-prototype occurrences.

### AS-03. UFRx schema and occurrence-tag branches

**Question.** Which UFRxDoc schemas outside 11 through 15 and which occurrence property/export tags have stable field grammars and semantic roles?

**Known.** Schemas 11 through 15 and the supported occurrence/export tag sets are framed. Other schemas and tags are refused as unsupported semantic branches; their containing UFRx records remain bounded and are reported without inventing fields.

Pre-schema-15 level-of-detail records are framed, but their member semantics are not transferred: kind 1 members and the kind 2 subassembly level-of-detail selection are retained natively. Which members a level of detail suppresses, and how the active level of detail is chosen, is unproven.

The weldment templates of the analyst corpus (`Weldment*.iam`, 40 saves) and three other saves store occurrence-table records that do not follow the occurrence grammar; they stay malformed UFRx records.

One analyst assembly save stores a `UFRxDoc` occurrence whose `occurrence_id` has no `AmDc` record while one `AmDc` record has no `UFRxDoc` occurrence, and whose `occurrence_key` repeats another occurrence's key. Its prototype resolves, but its placement stays unresolved; whether such a record is the replaced-component form of the other `AmDc` record is unproven.

The meaning of the eight export-entry state bytes, of the export padding byte, and of first-section tag `0x1e` (a second name that usually equals the component name) is unknown.

**Need.** Each additional schema or tag requires exact field widths, repeated-tag rules, identity joins, and loss behavior before semantic transfer.

**Note.** `parse_occurrence_export` chooses export branches from the first two u32 values and numeric thresholds (`count`, `next`, and `0xffff`) rather than a typed discriminator. A valid single-entry or future export form in the same numeric range can be consumed as the empty or alternate form.

### AS-02. Transform units and exceptional branches

**Question.** What semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. Placement translations use centimetres and are converted to neutral millimetres. No field in the external-reference prefix supplies an active placement.

**Need.** We must type the exceptional branches before transferring their scale, reflection, or singular-transform semantics.

### AS-04. Occurrence visibility and grounded state

**Question.** Which fields store the visibility and the grounded state of one assembly occurrence?

**Known.** External-reference state bit `0x2000` is the only decoded occurrence state. It marks a suppressed occurrence and sets `visible = false`. No other field sets `visible`.

The first `UFRxDoc` occurrence property section has the one-byte tags `0x07`, `0x0d`, `0x0f`, `0x10`, and `0x1d`. Tag `0x0f` and tag `0x10` change on one occurrence while all other occurrence, `AmDc`, and `AmGraphics` fields stay the same. Neither tag is visibility: the document preview shows the occurrence with each value. Tag `0x0f` is not the grounded state: an occurrence with tag `0x0f` set moves together with other occurrences when the constraint solution changes, and a one-occurrence assembly usually stores 0. Tag `0x0f` also stores values other than 0 and 1, for example `0x2a`, `0x37`, `0xd3`, and `0xff`. `AmDc` occurrence flag bits `0x00400000` and `0x00800000` change with no proven role. Bit `0x10` occurs only together with one of them.

Visibility is a property of a design-view representation. `DesignViewSegment` stores one record of type `41a2c29dd211cc99a0002aa976d456c9` for each representation, with a counted UTF-16LE name and a list of record references. Records of type `553a6b21d31123656000f587346bfbb0` contain a counted u32 list whose values are `UFRxDoc` occurrence keys, the first entry at the root. Records of types `2133049bd21187a9a0002ea976d456c9` and `2123c271d21187a9a0002ea976d456c9` store small state values. The active representation, the join from a state record to a key-path record, and the meaning of the state values are unknown.

**Need.** We must find the field for each state, and prove each value with two documents that differ only in that state, before we set `visible` from a representation or emit a grounded state. A grounded occurrence can map to a neutral `AssemblyJoint` with `JointOperands::Grounded` and one connector on the occurrence. The IR needs no addition for it.

### AS-05. Assembly constraint operands and neutral joints

**Question.** Which fields give the exact part geometry, the solution sense, and the remaining kind and state values of an assembly constraint, and how does a constraint become a neutral `AssemblyJoint`?

**Known.** Section 9.1 frames every constraint record, its kind, its operand records, and its parameter roles. Each operand gives an occurrence and a geometry record. The geometry record type gives the plane, line, point, or direction class. Part-side geometry records reference an `e5ae5e37d111e6f46000e5b1f1bc87b0` document record, and records of types `564d8790d011f8d10008cabc0663dc09`, `f32cee1dd111efb26000c8b1f1bc87b0`, and `61b8480cd2110541600093b7b035c3b0` reference them. The `564d8790` record stores a counted UTF-16LE key of the form `PmDCSegment::<segment>:<revision>:<n>`. Assembly origin planes, axes, and the center point are the plane, line, and point records that the first `634d8790d011f8d10008cabc0663dc09` record of the segment lists before the occurrences.

`state` is 0 on tangent constraints and -1 on the other kinds. `solution` stores 0, 1, 3, or 4. Kind 11 constraints have no browser entry and no parameter, and their geometry records have types `5f35cf76d511881c10000697994909b5`, `0a9476f0d41115f210000097994909b5`, `0b9476f0d41115f210000097994909b5`, and `2149d0ea714709f6e477c49b46f25ca8`. Insert operands use geometry type `6a71439ed211a50f600084b7b035c3b0`. One tangent operand uses geometry type `9eda1641d11197bf6000b2b801f31bb0`. A small number of operands select records of types `09fcae1ce64010d04d85b8aefb05d6f1` and `b861c7f2fd4979186a13dc91f4bc3124` instead of an operand record. Design Accelerator assemblies store constraints with no browser entry.

Section 9.2 decodes the selection keys: a part face, a part face axis, a part or subassembly work feature by source index, or a geometry record of the assembly itself. Imported-face naming records give integer groups that equal a face's `NMx_Import_Brep_tag` attribute. Section 9.3 decodes the operand geometry records into the selected plane, line, point, or direction in the coordinates of the referenced document. That grammar is established only at segment major versions 23 and 25. The geometry records of constraints without a browser entry (types `5f35cf76…`, `0a9476f0…`, `0b9476f0…`, `2149d0ea…`) are not decoded. Work-feature keys with source indexes 4, 5, and 6 select part origin planes whose stored axes equal the cached plane directions; other source indexes are not checked.

Section 9.4 projects the constraints with `solution` 0 into neutral joints. Constraints with `solution` 4 are not satisfied by the stored placements: of the plane mates and flushes with `solution` 4, one in eleven meets its relation.

**Need.** We must decode the naming-record types of modeled parts (section 9.2) into face and edge identities and establish the `<revision>` and `<number>` key components. We must establish what `solution` values 1, 3, and 4 state (a suppressed constraint, an unsolved constraint, inside against outside tangency) with controlled saves. The limit order (which of high bytes 1 and 2 is the minimum) is unproven, so constraints with limits remain native. A line mate with a nonzero value, and angle and point operands without a selection key, have no proven relation and remain native. The tangent, insert, rotation-motion, and symmetry families have no neutral joint family.

## 4. Properties, materials, and design intent

### PR-01. OLE property variants and code pages

**Question.** Which additional OLE code pages, property value types, section-directory variants, and preview payloads occur in Inventor documents, and what are their exact mappings?

**Known.** The current parser handles the supported little-endian property-set header, section ranges, scalar/vector/FILETIME/BLOB/clipboard forms, UTF-16, and the admitted `encoding_rs` code-page set. Unknown typed values remain native-only; an unsupported code page or property variant stops typed property projection after its stream has been identified.

**Need.** Additional code pages, typed variants, and preview encodings require exact decoding, range validation, and neutral/native mapping rules before transfer.

**Note.** When a property section has no code-page property, LPSTR and dictionary strings are decoded as Windows-1252. The format model does not establish that an absent code page selects Windows-1252.

### PR-02. Document kind and remaining property identities

**Question.** Which property identity selects the document kind, and which Inventor Summary Information property IDs store the subject, keywords, creation time, and last-save time?

**Known.** Section 6.1 settles the identity and precedence of fifteen neutral metadata keys. `MetadataProjection` still selects the document kind from a property whose normalized name is `documentkind` or `documenttype`, and it replaces the document kind whenever another parseable value occurs. Design Tracking Properties property 32 stores a document-type string, but its observed values do not name a part or an assembly. Inventor Summary Information properties 3, 5, 12, and 13 have no nonempty value in the inspected documents. Design Tracking Properties property 40 stores a signed design-status code, and its code-to-label map is not known.

**Need.** The document kind requires one FMTID and property identity and a conflict rule before it can override the RSe document kind. The subject, keywords, creation-time, and save-time keys can admit Inventor Summary Information identities only after a document with a nonempty value proves each property ID.

### MA-01. Additional face appearance styles

**Question.** Which `PmGraphics` or `FBAttribute` style families bind a Protein appearance asset, texture, or non-diffuse presentation channel to a face?

**Known.** The `PmApp` document-default record selects one rendering-style record by a carrier-local one-based record reference. The rendering style stores the Protein asset GUID and asset-library identifier. Their unique catalog join supplies the default appearance for every part body. A PmGraphics face joins a transferred ASM face through the shared nonnegative Design key. Its object-style collection can select one primary-color style whose second RGBA vector is a direct diffuse face override. The face override has precedence over the body default.

**Need.** Each additional style family requires an exact owner path, channel semantics, and precedence before transfer.

**Note.** The implementation selects `PmGraphicsPrimaryColorStyle.colors[1]` as the diffuse face color without a record discriminator for the four color slots. The slot role remains part of the open channel-semantics question. A current rendering style without a catalog asset supplies its inline diffuse color as the body default. Some rendering styles that no body selects store a 191-byte suffix with `style_values[0] = 1` whose inline colors do not follow the style label. Whether that form marks an unmaterialized library style, and so whether its inline colors are valid when a face or body selects it, is unknown. The fields after the inline lighting block, the role of the ambient, specular, and emissive vectors, and the `PmGraphics` style-collection members that reference neither a primary-color style nor a rendering style are also unknown.

### MA-02. Protein material-property identity and precedence

**Question.** Which schema-qualified Protein property owns each material scalar, bitmap URI, bitmap URN, base-color channel, and texture-mapping value, and what value does each schema define when a property is absent?

**Known.** Material projection scans matching property IDs and accepts the first match for texture mapping values, bitmap paths, bitmap URNs, and normalized color candidates. It also maps `generic_refraction_index` and `transparent_refraction_index` to one neutral key, so a later map insertion replaces the earlier value.

**Need.** Schema declarations must establish property ownership, channel precedence, ambiguity behavior, and omission semantics before material values can be transferred.

**Note.** The current projection supplies schema-independent values when mapping properties are absent: map channel 1, UVW source 0, zero offsets and rotation, unit scale, enabled repeat, zero real-world mapping values and bump depth, and unit normal scale. These values are not yet established as the omission semantics of each Protein schema.

### MA-03. Sheet-metal rule and unfold-rule field roles

**Question.** Which sheet-metal rule parameter does each rule text and value set, which unfold method does each `method` value select, and what do the unfold-rule secondary and tertiary factors, scalars, states, and lists encode?

**Known.** The record grammar and the unfold-rule and material-style references are exact. The rule texts are unit-qualified expressions in terms of `#Thickness#` and `#BendRadius#`, and text 7 is the only literal length. The document sheet-metal parameters `Thickness`, `KFactor`, `BendRadius`, `BendReliefWidth`, `BendReliefDepth`, `CornerReliefSize`, `SeamGap`, `MinimumRemnant`, `TransitionRadius`, `JacobiRadiusSize`, and `GapSize` are PmDc document parameters. The document property `Sheet Metal Rule` names the active rule.

**Need.** Controlled saves that change one rule field at a time must assign each slot before the rule can transfer as neutral sheet-metal defaults or select the active rule from bytes alone.

### DE-01. Feature and history record graph

**Question.** Which typed records define history order, feature operands, suppression, and result-body transitions?

**Known.** Feature containers, root labels, and group labels define the browser history order, the end-of-part position, and the flat-pattern history. Content-header flag bits define suppression and the below-end-of-part state. Feature labels identify consumed sketches, and pattern-feature participants identify seed features. Numeric PmDc parameters, planar sketches, generic feature records, rectangular-pattern records, mirror records, and end-of-features records have stable carrier-qualified identities. Generic, rectangular-pattern, and mirror records supply ordered property-reference vectors. Pattern records also supply ordered participant references. Typed feature properties identify part-operation, extent, hole, fillet, chamfer, Boolean, boundary-patch, feature-dimension, object-collection, fillet-edge-set, edge-collection, edge-item, body, profile-selection, and placement records. Closed one-sided, symmetric, two-sided, to, and from-to extrusion, drilled/countersink/counterbore hole, constant-radius edge-fillet, and equal-distance edge-chamfer branches have exact dimensions, native selections, placements, and native result-body identities. Feature-label records supply an owner, a label, a class identifier, and ordered participant references. Entity-style links retain associative entity identifiers and entity types. Planar point, line, circle, ellipse, placement, geometric-constraint, and dimensional-constraint graphs have exact record references and neutral identities. Exact RSe frames provide type identifiers and record ranges for the remaining design records.

**Need.** Body-level evaluation dependencies, current-model body joins, rectangular and circular pattern transforms, the thicken side Boolean in slot 2, split forms other than form 3, moves with a rotation and the face-list move form, body mirrors with a true slot-7 Boolean and the meaning of the mirror slot-7 and slot-11 Booleans, revolve operands, extrusion targets other than work planes and plane faces, the meaning of the termination flag, flat-pattern definition records, non-default fillet laws, oriented chamfers, and the remaining operation families require exact record joins before transfer. These fields must resolve before the score advances to L4.

**Conflict.** The specification states that the slot-16 direction record supplies the drilling direction of a hole with a slot-21 placement, and the decoder uses it unchanged. A sketch-point hole drills opposite the same slot-16 record, which is the sketch direction. A decision must establish whether placement holes also drill opposite their direction record.

### DE-02. Compound parameter units and function expressions

**Question.** Which PmDc unit graphs and function records define compound dimensions, derived units, offsets, and function-call expression text?

**Known.** A scalar parameter unit with one supported numerator, no denominator, and no derived-unit reference transfers with exact length, angle, or dimensionless semantics. Literal, parameter-reference, unary, and binary arithmetic expression records form a closed ordered graph.

**Need.** Compound or derived units and function calls require exact dimensional and textual semantics before neutral transfer.

### DE-03. Remaining sketch records and semantics

**Question.** What does the planar-sketch `count_value` count, what do entity-flag bits other than `0x00080000` and `0x00040000` mean, which field marks a driven (reference) dimension, what does the key of a selector-5 spline select, and which grammars define inserted sketch images and the remaining construct records?

**Known.** Points, lines with on-curve points, circles, counterclockwise arcs, complete ellipses, stored B-splines, sketch-block definitions, instances, and proxies, sketch mirror nodes, and the relations and dimensions of the planar-sketch tables have exact operands. Constraint maps store curve parameters and endpoint selectors. Bit `0x00000040` is set on projected entities and on most solved entities; it is clear on some entities that remain free. Bits `0x00200000` and `0x00008000` occur on lines. Selector-5 spline keys are consecutive within one projected chain. Record `08db93dcc149f26ff229ecb1804d9784` stores a sketch reference without entity flags, four f64 values, a u32 value, a compact 4-by-4 matrix, and a tail; records `0ead987ab9495ceefc186787a2a20643` and `5f33ec8f6c4fb6be9fcc71963ad1b9df` reference its four frame lines and points. Records `7f341025ae4a79961d9047869d5f4e48` and `013cbb1fd2114d68a0009e9a3c3aa076` occur in sketch entity arrays. Projection links name a model origin, vertex reference, or edge item; the vertex and edge references continue through records `107388c1d211b93e8000cdb1e13554c7`, `70efdbb8d211c33e8000cdb1e13554c7`, and `534d8790d011f8d10008cabc0663dc09`. Feature profile selections reference `21750fccd1112780e38619962259017a` region records.

**Need.** The count requires an exact referent before validation uses it. A dimension's driving state requires its flag before transfer of `driving`. A selector-5 spline needs its model-curve join before its geometry transfers. Each image and construct record requires its complete field grammar before its sketch closes. The model-reference chains of projection links and the region records require their model-topology joins before projected geometry binds a model edge or vertex and before feature region selection transfers.

### DE-04. Parameter tables without a model owner

**Question.** Which record owns a PmDc parameter table whose context reference is null, and what scope applies to its parameters?

**Known.** These tables have flags `0x20` and two-entry member lists. No record references the table. Their parameters are referenced by records of type `b038b999d211c35b6000b3856c2387b0`. Their parameters remain native with a parameter-scope loss.

**Need.** The owning record and its feature or sketch identity must be exact before these parameters can enter a neutral scope.

### DE-05. Model-root and parameter-table fields

**Question.** What do the model-root flags, the model-root values other than values 3 and 9, the model-root `scale` and `terminal_value`, the parameter-table member list, and the parameter-table suffix encode?

**Known.** Value 3 references the root's parameter table and value 9 is null or repeats it. Value 12 is 1 in document model roots and 7 in flat-pattern model roots. The member list and the name map contain every parameter of the table and also other records. The suffix is retained by length.

**Need.** The model graph, the flat-pattern definition operands, and the table membership rule must be exact before the flat-pattern model can transfer more than its parameter scope.

### DE-06. Sheet-metal bends, corners, and flange definitions

**Question.** Which fields of the bend, bend-relief, bend-settings, corner, and corner-edge records define the bend radius, the bend and corner reliefs, and the bend and corner edges, what does plate slot 1 of a flange plate encode, and which record defines the flange height, angle, and edge?

**Known.** Face and flange group records list their plate, bend, and corner members. The plate projects to its wall. The bend and corner members share the plate's bend, relief, settings, corner, and corner-edge records. The bend edge collection of a plate is empty. A flange plate's slot-1 direction record is parallel to its sketch plane. Record `aaa0f06bd311a5976000a7a99dccefb0` references the flange parameters, an edge item, the flange sketch points, and the plate's region selection, direction, and flip Boolean; no feature or group record references it.

**Need.** The bend and corner features stay native until their geometry has exact operands. A flange can transfer as one edge flange only when its height, angle, edge, height datum, bend position, and width are exact.

### DE-07. Edge-item and region-selection model joins

**Question.** What does the value of an edge item's index-reference list select, and how does a kernel-entity link's entity identifier join a region face to the loops of its sketch?

**Known.** A referencing edge item has one index-reference value. The second list-metadata value is `0x10000000`. The value is not a record reference of the PmDc segment. The edges that a fillet, corner round, or chamfer selects are replaced by the operation, so the stored part body does not contain them. The body reference of a kernel-entity link selects a PmDc record of type `5c5945f6d5113313100060a6bba647b5`; its kernel bytes are an `ASM BinaryFile4` body with `NamingMatching` attributes and a history stream.

**Need.** A neutral edge selection needs the edge in an input topology of the consuming feature. A region selection transfers as sketch regions only when the region face of the kernel body joins exact sketch loops.

### DE-08. Design records at other segment major versions

**Question.** Which content-header extension, point tail, curve auxiliary-list, and record-tail forms apply at segment major version 24 and at versions after 25, and what do the content-header extension values and the version-25 point list encode?

**Known.** Segment major versions 15 through 23 and 25 have exact design-record grammars. The extension values of version-23 and version-25 headers are -1, 0, or positive integers. The version-25 point list is empty in most records.

**Need.** Each further version requires its header extension length and per-class header form before its design records parse. The feature property vectors of several classes grow at segment major version 23: extrusion features store 31 slots instead of 30 (the added slot is null), sheet-metal plates 17 instead of 15 with a different slot-2 record type and two added trailing slots, the feature-label class `1a7d751f…` 28 instead of 26, and class `f258f74c…` 11 instead of 9. The plate projection and the projections of those classes require their version-23 slot maps.

**Note.** A revolution feature (label class `5f015400d211251c600067b79b49ebb0`) stores 20 slots: part operation, boundary patch, axis record `896cf08ed1113c0460007cb801f31bb0`, extent enumeration, angle parameter, record `c26aa0c728412312abf0728f9d598bc9`, feature dimensions, an auxiliary enumeration, Booleans, and the result collection. The axis record stores, after its content header, four u32 values, an empty type-2 list, two f64 triples (the axis segment's endpoints in internal centimetres), and a u8. The extent values and the `c26aa0c7` value that select a full or one-sided revolution and its turn direction are not yet separated.

### DE-09. Version-25 spline points and spline auxiliary lists

**Question.** What follows the center-of list of a version-25 point whose state is 2, and which gate or list follows the point list of a version-25 spline?

**Known.** A state-2 point stores a type-2 preamble with count 2 followed by two f64 values, a second type-2 preamble with count 2, an empty type-2 list, a reference, an empty type-2 list, and a reference. The points occur with sketch splines. A version-25 spline stores `[0, 0]` where earlier versions store the auxiliary list.

**Need.** The state-2 tail and the spline gate require exact field grammars before these records and their sketches close.

## 5. Adjacent envelopes

### EN-01. Other CFB and RSe versions

**Question.** Which RSe schema, metadata, record-trailer, and carrier variants accompany CFB version 4 or metadata versions other than 8?

**Known.** The compound reader admits valid CFB version 3 and 4 containers. The Inventor semantic envelope is schema 31 and metadata version 8. Those grammars are applied to every document, whatever it declares; a declaration outside them leaves the document dialect-unverified.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Drawing and presentation documents

**Question.** Which segment families and records authoritatively identify and define IDW and IPN document semantics?

**Known.** Property metadata can name drawing and presentation kinds. Their semantic decode is not part of the IPT/IAM envelope.

**Need.** Future family support must extend the Inventor codec without treating these documents as parts or assemblies.
