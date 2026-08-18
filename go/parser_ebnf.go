// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// parser_ebnf.go — the EBNF front-end: EBNF text in, grammar IR out.
//
// Everything downstream of the IR belongs to github.com/tabnas/bnf/go
// and is shared with the ABNF and GBNF front-ends:
//
//	EBNF text --ParseEbnf--> bnf.Grammar --bnf.EmitGrammarSpec--> GrammarSpec
//
// WHICH EBNF. The dialect is the same documented choice the TypeScript
// front-end makes, and ts/README.md's supported/not-supported table is
// the specification both implementations answer to: W3C EBNF primarily
// (`::=`, postfix `? * +`, `|`, `( … )`, case-SENSITIVE literals,
// `[a-z]` / `[^<&]` / `[#x20-#x7E]` classes, standalone `#xNN`,
// `/* … */`), plus the four ISO 14977 spellings that cannot collide
// (`=`, `,`, `;`, `(* … *)`).
//
// A DELIBERATE DIVERGENCE FROM THE TS FRONT-END, stated plainly because
// it is the kind of thing that should not be discovered later: the
// TypeScript parser expresses EBNF as a tabnas rule table and parses it
// with the engine itself. This one is a hand-written recursive-descent
// scanner. The IR is the contract between a front-end and the compiler,
// and the two implementations are held to the same accept/reject table
// by their tests — but the mechanisms differ, so a defect in one will
// not automatically appear in the other. Re-expressing this as a rule
// table (as the ABNF Go port does) would close that gap.
package ebnf

import (
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"

	bnf "github.com/tabnas/bnf/go"
)

// A production name (W3C calls it a symbol). Deliberately narrower than
// "whatever ran to the next delimiter": without this a typo like
// `Foo! ::= …` would become a rule genuinely named `Foo!`, and the
// mistake would only surface much later as an unknown-rule reference.
func validName(s string) bool {
	if s == "" {
		return false
	}
	for i, r := range s {
		switch {
		case r >= 'A' && r <= 'Z', r >= 'a' && r <= 'z', r == '_':
		case i > 0 && (r >= '0' && r <= '9' || r == '.' || r == '-'):
		default:
			return false
		}
	}
	return true
}

// scanner walks EBNF source one rune at a time, tracking line/column so
// every diagnostic can say where it is.
type scanner struct {
	src  string
	i    int
	line int
	col  int
}

func newScanner(src string) *scanner {
	return &scanner{src: src, line: 1, col: 1}
}

func (s *scanner) eof() bool { return s.i >= len(s.src) }

func (s *scanner) peek() byte {
	if s.eof() {
		return 0
	}
	return s.src[s.i]
}

func (s *scanner) advance(n int) {
	for k := 0; k < n && !s.eof(); k++ {
		if s.src[s.i] == '\n' {
			s.line++
			s.col = 1
		} else {
			s.col++
		}
		s.i++
	}
}

func (s *scanner) has(lit string) bool {
	return strings.HasPrefix(s.src[s.i:], lit)
}

// ---- Source spans --------------------------------------------------
//
// Where each IR node came from in the source, so a compile failure can
// carry a range and a tool can underline the offending text (see
// bnf.SrcSpan). A srcMark is the scanner's position at the START of a
// node; spanFrom pairs it with the position once the node has been
// consumed. That is the whole mechanism — a node's span is the stretch
// of source the scanner walked to build it, so `src[Sp.S:Sp.E]` is that
// node's text and nothing else.
//
// This is one more place the two front-ends necessarily differ (see the
// divergence note at the top of this file). The TypeScript front-end is
// a rule table, so it copies `sI`/`rI`/`cI` straight off the matched
// TOKEN; this one has no tokens, so it reads the scanner's own counters
// instead. The two agree on the part the IR contract cares about —
// WHICH text each node spans: a string terminal covers its quotes, a
// group covers `(` through `)`, a production covers its name alone.
//
// UNITS are runtime-native and deliberately unconverted, exactly as
// bnf.SrcSpan specifies: these offsets count BYTES where TypeScript's
// count UTF-16 code units. Note that C counts bytes too, because
// `advance` steps one byte at a time — that is a PRE-EXISTING
// divergence from the TS column (which counts UTF-16 units), shared
// with the Line/Column this scanner already reports on ParseError, and
// deliberately not changed here.
type srcMark struct {
	i    int
	line int
	col  int
}

// mark records where a node starts, before the scanner consumes it.
func (s *scanner) mark() srcMark {
	return srcMark{i: s.i, line: s.line, col: s.col}
}

// spanFrom is the span from a mark to wherever the scanner has reached.
func (s *scanner) spanFrom(m srcMark) *bnf.SrcSpan {
	return &bnf.SrcSpan{S: m.i, E: s.i, R: m.line, C: m.col}
}

func (s *scanner) errf(format string, args ...any) *ParseError {
	return &ParseError{
		Message: fmt.Sprintf("ebnf: "+format, args...) +
			fmt.Sprintf(" at line %d, column %d", s.line, s.col),
		Line:   s.line,
		Column: s.col,
	}
}

// skipSpace consumes whitespace and both comment syntaxes. ISO's
// `(* … *)` has to be recognised here rather than as a lexer comment
// definition, because `(` also opens a group — the two only separate on
// the second character.
func (s *scanner) skipSpace() error {
	for !s.eof() {
		c := s.peek()
		if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
			s.advance(1)
			continue
		}
		if s.has("/*") {
			end := strings.Index(s.src[s.i+2:], "*/")
			if end < 0 {
				return s.errf("unterminated comment '/*'")
			}
			s.advance(2 + end + 2)
			continue
		}
		if s.has("(*") {
			end := strings.Index(s.src[s.i+2:], "*)")
			if end < 0 {
				return s.errf("unterminated comment '(*'")
			}
			s.advance(2 + end + 2)
			continue
		}
		return nil
	}
	return nil
}

// ---- Terminals -----------------------------------------------------

// codePoint reads a hex code point, refusing anything Unicode does not
// have. Mirrors the TS `codePoint`.
func (s *scanner) codePoint(hex, whole string) (rune, error) {
	n, err := strconv.ParseInt(hex, 16, 64)
	if err != nil {
		return 0, s.errf("'%s' is not a valid code point", whole)
	}
	if n > 0x10FFFF {
		return 0, s.errf(
			"code point '%s' is above the Unicode maximum (the maximum is #x10FFFF)",
			whole)
	}
	return rune(n), nil
}

// classPart is one member or range of a character class.
type classPart struct{ lo, hi rune }

func classEscape(cp rune, astral bool) string {
	// Go's RE2 spells a code point `\x{…}`; `\uXXXX` is the JavaScript
	// spelling and RE2 refuses to compile it. The shared compiler
	// converts `\x{…}` to `\uXXXX` when serialising a spec for the
	// other runtime, so this is the right native form here — the two
	// runtimes agree on the language, not on the source text.
	//
	// `astral` is therefore unused on this side: `\x{…}` reaches every
	// code point, where JavaScript needs `\u{…}` and the `u` flag above
	// the BMP. Kept in the signature so the two implementations stay
	// readable side by side.
	_ = astral
	return "\\x{" + strconv.FormatInt(int64(cp), 16) + "}"
}

// parseCharClass lowers `[a-z]`, `[^<&]`, `[#x20-#x7E]` and friends onto
// a regex element. Faithful to the TS implementation, including the two
// rules that are easy to get wrong: a `-` that is not between two
// members is a literal hyphen (W3C grammars rely on this for `[-+]`),
// and members are read by code point rather than by UTF-16 unit.
func (s *scanner) parseCharClass() (*bnf.Element, error) {
	m := s.mark()
	start := m.i
	s.advance(1) // consume `[`
	negated := false
	if s.peek() == '^' {
		negated = true
		s.advance(1)
	}
	bodyStart := s.i
	for !s.eof() && s.peek() != ']' && s.peek() != '\n' {
		s.advance(1)
	}
	if s.eof() || s.peek() != ']' {
		return nil, s.errf("unclosed character class '%s'",
			s.src[start:min(len(s.src), start+20)])
	}
	body := s.src[bodyStart:s.i]
	s.advance(1) // consume `]`
	whole := s.src[start:s.i]

	if body == "" {
		return nil, s.errf("empty character class '%s' matches nothing", whole)
	}

	// Members in source order. A hyphen is kept as a marker rather than a
	// code point, so the fold below can tell `a-z` from `[-a]`.
	const hyphenMark rune = -1
	var items []rune
	for i := 0; i < len(body); {
		// `#xNN` — the W3C spelling inside a class. Lowercase `x` only,
		// matching the standalone form and the documented dialect.
		if body[i] == '#' && i+1 < len(body) && body[i+1] == 'x' {
			j := i + 2
			for j < len(body) && isHexDigit(body[j]) {
				j++
			}
			if j == i+2 {
				return nil, s.errf("'#x' with no hex digits in character class '%s'",
					whole)
			}
			cp, err := s.codePoint(body[i+2:j], body[i:j])
			if err != nil {
				return nil, err
			}
			items = append(items, cp)
			i = j
			continue
		}
		if body[i] == '-' {
			items = append(items, hyphenMark)
			i++
			continue
		}
		r, size := utf8.DecodeRuneInString(body[i:])
		items = append(items, r)
		i += size
	}

	var parts []classPart
	for k := 0; k < len(items); k++ {
		if items[k] == hyphenMark {
			parts = append(parts, classPart{0x2D, 0x2D})
			continue
		}
		if k+2 < len(items) && items[k+1] == hyphenMark &&
			items[k+2] != hyphenMark {
			hi := items[k+2]
			if hi < items[k] {
				return nil, s.errf(
					"reversed range in character class '%s' — the low end must not "+
						"be greater than the high end", whole)
			}
			parts = append(parts, classPart{items[k], hi})
			k += 2
			continue
		}
		parts = append(parts, classPart{items[k], items[k]})
	}

	astral := false
	for _, p := range parts {
		if p.lo > 0xFFFF || p.hi > 0xFFFF {
			astral = true
		}
	}

	var b strings.Builder
	b.WriteString("[")
	if negated {
		b.WriteString("^")
	}
	for _, p := range parts {
		if p.lo == p.hi {
			b.WriteString(classEscape(p.lo, astral))
		} else {
			b.WriteString(classEscape(p.lo, astral))
			b.WriteString("-")
			b.WriteString(classEscape(p.hi, astral))
		}
	}
	b.WriteString("]")

	// Unicode mode follows what the matcher can MATCH, not what was
	// written: a negated class matches the complement of its members, and
	// that complement always contains every astral code point.
	flags := ""
	if negated || astral {
		flags = "u"
	}
	// The whole class as written, `[` through `]` — `whole` above is the
	// same text, which is what the diagnostics here quote.
	return &bnf.Element{
		Kind: bnf.KindRegex, Pattern: b.String(), Flags: flags,
		Sp: s.spanFrom(m),
	}, nil
}

func isHexDigit(c byte) bool {
	return c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F'
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ---- Named rejections ----------------------------------------------
//
// Every construct this dialect refuses raises an error naming it and its
// position, rather than compiling to the wrong language. The set matches
// ts/README.md's not-supported table exactly.

func (s *scanner) failSubtraction() error {
	return s.errf("subtraction ('-') is not supported. The grammar IR has no " +
		"difference operator, so 'A - B' cannot be compiled. Where both sides " +
		"are single characters, write the difference as a negated character " +
		"class instead — '[^abc]' rather than 'Char - [abc]'")
}

func (s *scanner) failSpecialSequence() error {
	return s.errf("ISO 14977 special sequences ('? … ?') are not supported: " +
		"their content is deliberately undefined by the standard, so there is " +
		"nothing to compile")
}

func (s *scanner) failIsoRepetition(open byte) error {
	return s.errf("ISO 14977 bracket repetition ('%c') is not supported; this "+
		"dialect spells repetition postfix — 'A*' rather than '{ A }'", open)
}

func (s *scanner) failEmptyAlternative() error {
	return s.errf("empty alternative. Both dialects require an expression on " +
		"each side of '|', and this notation has no epsilon terminal; to make " +
		"a construct optional write 'A?' rather than 'A |'")
}

func (s *scanner) failLeadingComma() error {
	return s.errf("leading comma. In ISO 14977 the comma separates " +
		"concatenated items, so it must appear between two of them — " +
		"'A , B', not ', A'")
}

// ---- Recursive descent ---------------------------------------------

// parseAtom reads one atom: a name, a literal, a class, a code point, or
// a parenthesised alternation.
func (s *scanner) parseAtom() (*bnf.Element, error) {
	if err := s.skipSpace(); err != nil {
		return nil, err
	}
	if s.eof() {
		return nil, s.errf("expected an expression")
	}
	c := s.peek()
	// Where this atom starts, leading space and comments already behind
	// us. Every span below runs from here to wherever the scanner has
	// reached once the atom has been consumed.
	m := s.mark()

	switch {
	case c == '"' || c == '\'':
		quote := c
		s.advance(1)
		start := s.i
		for !s.eof() && s.peek() != quote {
			s.advance(1)
		}
		if s.eof() {
			return nil, s.errf("unterminated string literal")
		}
		lit := s.src[start:s.i]
		s.advance(1)
		if lit == "" {
			return nil, s.errf("empty string literal matches nothing; this " +
				"notation has no epsilon terminal")
		}
		// W3C literals are case-SENSITIVE — the opposite of RFC 5234.
		//
		// The span covers the QUOTES as well as the literal, matching TS:
		// what the author wrote is `"hi"`, and that is what a diagnostic
		// should underline. `lit` deliberately does not.
		return &bnf.Element{
			Kind: bnf.KindTerm, Literal: lit,
			CaseSensitive: true, HasCaseSens: true,
			Sp: s.spanFrom(m),
		}, nil

	case c == '[':
		return s.parseCharClass()

	case c == '(':
		s.advance(1)
		alts, err := s.parseAlts()
		if err != nil {
			return nil, err
		}
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		if s.peek() != ')' {
			return nil, s.errf("unclosed group '('")
		}
		s.advance(1)
		// The opening paren through the closing one — TS's `spanTo(lp,
		// rp)`. The elements inside carry their own, narrower spans.
		return &bnf.Element{
			Kind: bnf.KindGroup, Alts: alts,
			Sp: s.spanFrom(m),
		}, nil

	case c == '#':
		if !s.has("#x") {
			return nil, s.errf("expected '#x' followed by hex digits")
		}
		s.advance(2)
		start := s.i
		for !s.eof() && isHexDigit(s.peek()) {
			s.advance(1)
		}
		if s.i == start {
			return nil, s.errf("'#x' with no hex digits")
		}
		hex := s.src[start:s.i]
		cp, err := s.codePoint(hex, "#x"+hex)
		if err != nil {
			return nil, err
		}
		// `#x41` as written, not the single character it denotes.
		return &bnf.Element{
			Kind: bnf.KindTerm, Literal: string(cp),
			CaseSensitive: true, HasCaseSens: true,
			Sp: s.spanFrom(m),
		}, nil

	case c == '?':
		return nil, s.failSpecialSequence()
	case c == '-':
		return nil, s.failSubtraction()
	case c == '{' || c == '}':
		return nil, s.failIsoRepetition(c)
	case c == '*' || c == '+':
		return nil, s.errf("repetition in this dialect is postfix, so '%c' "+
			"must follow the thing it repeats", c)

	default:
		start := s.i
		for !s.eof() && isNameByte(s.peek()) {
			s.advance(1)
		}
		if s.i == start {
			return nil, s.errf("unexpected character %q", string(c))
		}
		name := s.src[start:s.i]
		if !validName(name) {
			return nil, s.errf("'%s' is not a valid symbol name", name)
		}
		return &bnf.Element{Kind: bnf.KindRef, Name: name, Sp: s.spanFrom(m)}, nil
	}
}

func isNameByte(c byte) bool {
	return c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' ||
		c >= '0' && c <= '9' || c == '_' || c == '.' || c == '-'
}

// parseElem reads an atom plus any run of postfix operators, applied
// left to right so `(x)*?` is `((x)*)?` — outermost last, matching TS.
//
// The wrappers below carry NO span, deliberately: the TS front-end
// stamps its five atom kinds and nothing else, and the two ports have
// to agree on which nodes carry spans, not just on their extents. The
// atom inside keeps its own span, so `A*` still locates `A`. If these
// should span the operator too, that is a change to make in TypeScript
// first — it is canonical here.
func (s *scanner) parseElem() (*bnf.Element, error) {
	el, err := s.parseAtom()
	if err != nil {
		return nil, err
	}
	for {
		if s.eof() {
			return el, nil
		}
		switch s.peek() {
		case '?':
			s.advance(1)
			el = &bnf.Element{Kind: bnf.KindOpt, Inner: el}
		case '*':
			s.advance(1)
			el = &bnf.Element{Kind: bnf.KindStar, Inner: el}
		case '+':
			s.advance(1)
			el = &bnf.Element{Kind: bnf.KindPlus, Inner: el}
		default:
			return el, nil
		}
	}
}

// atSeqEnd reports whether the scanner is at something that ends the
// current sequence rather than continuing it.
func (s *scanner) atSeqEnd() bool {
	if s.eof() {
		return true
	}
	switch s.peek() {
	case '|', ')', ';':
		return true
	}
	// A new production starts here: NAME followed by `::=` or `=`.
	save := *s
	defer func() { *s = save }()
	start := s.i
	for !s.eof() && isNameByte(s.peek()) {
		s.advance(1)
	}
	if s.i == start {
		return false
	}
	if err := s.skipSpace(); err != nil {
		return false
	}
	return s.has("::=") || (s.peek() == '=' && !s.has("=="))
}

// parseSeq reads a concatenation. `first` distinguishes the opening
// position, where a terminator or a comma means the alternative is empty
// or starts with a separator — neither of which either dialect allows.
func (s *scanner) parseSeq() (bnf.Sequence, error) {
	var seq bnf.Sequence
	first := true
	for {
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		if s.atSeqEnd() {
			if first {
				return nil, s.failEmptyAlternative()
			}
			return seq, nil
		}
		if s.peek() == ',' {
			if first {
				return nil, s.failLeadingComma()
			}
			s.advance(1)
			if err := s.skipSpace(); err != nil {
				return nil, err
			}
			if s.atSeqEnd() {
				return nil, s.errf("trailing comma with nothing after it")
			}
		}
		el, err := s.parseElem()
		if err != nil {
			return nil, err
		}
		seq = append(seq, el)
		first = false
	}
}

func (s *scanner) parseAlts() ([]bnf.Sequence, error) {
	var alts []bnf.Sequence
	for {
		seq, err := s.parseSeq()
		if err != nil {
			return nil, err
		}
		alts = append(alts, seq)
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		if s.peek() != '|' {
			return alts, nil
		}
		s.advance(1)
	}
}

// ---- Grammar-level parse and validation ----------------------------

// ParseEbnf reads EBNF source into the shared grammar IR.
func ParseEbnf(src string) (*bnf.Grammar, error) {
	s := newScanner(src)
	var prods []*bnf.Production

	for {
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		if s.eof() {
			break
		}
		m := s.mark()
		start := m.i
		for !s.eof() && isNameByte(s.peek()) {
			s.advance(1)
		}
		if s.i == start {
			return nil, s.errf("expected a rule name")
		}
		name := s.src[start:s.i]
		// The production's span is its NAME, taken here while the scanner
		// still sits at the end of it. A rule's body can run over many
		// lines, and every consumer of this span — an outline entry,
		// go-to-definition, the underline on a whole-rule diagnostic like
		// "purely left-recursive" — wants the name, not the paragraph.
		// Element spans already locate everything inside. Matches TS.
		nameSp := s.spanFrom(m)
		if !validName(name) {
			return nil, s.errf("'%s' is not a valid symbol name", name)
		}
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		switch {
		case s.has("::="):
			s.advance(3)
		case s.peek() == '=':
			s.advance(1)
		default:
			return nil, s.errf(
				"expected '::=' or '=' after rule name '%s'", name)
		}
		alts, err := s.parseAlts()
		if err != nil {
			return nil, err
		}
		prods = append(prods, &bnf.Production{Name: name, Alts: alts, Sp: nameSp})

		// ISO's `;` terminator is optional and carries no meaning here.
		if err := s.skipSpace(); err != nil {
			return nil, err
		}
		if !s.eof() && s.peek() == ';' {
			s.advance(1)
		}
	}

	if len(prods) == 0 {
		return nil, &ParseError{Message: "ebnf: grammar defines no rules"}
	}
	if err := checkDuplicates(prods); err != nil {
		return nil, err
	}
	if err := checkNullableAlts(prods); err != nil {
		return nil, err
	}
	return &bnf.Grammar{Productions: prods}, nil
}

// EBNF has no incremental-alternatives operator, so a name defined twice
// is a mistake rather than an extension.
func checkDuplicates(prods []*bnf.Production) error {
	seen := map[string]bool{}
	for _, p := range prods {
		if seen[p.Name] {
			return &ParseError{Message: fmt.Sprintf(
				"ebnf: rule '%s' is defined more than once; EBNF has no "+
					"incremental-alternatives operator", p.Name)}
		}
		seen[p.Name] = true
	}
	return nil
}

// checkNullableAlts refuses the one ambiguity this front-end can rule
// out soundly and cheaply: two alternatives that both derive the empty
// string. No lookahead can separate them, because there is nothing to
// look at, and the shared compiler emits a dispatch anyway and then
// mis-parses. This is NOT a general ambiguity check.
func checkNullableAlts(prods []*bnf.Production) error {
	nullable := map[string]bool{}

	var elNullable func(el *bnf.Element) bool
	elNullable = func(el *bnf.Element) bool {
		switch el.Kind {
		case bnf.KindOpt, bnf.KindStar:
			return true
		case bnf.KindPlus:
			return elNullable(el.Inner)
		case bnf.KindRep:
			return el.Min == 0
		case bnf.KindGroup:
			for _, a := range el.Alts {
				if altNullable(a, elNullable) {
					return true
				}
			}
			return false
		case bnf.KindRef:
			return nullable[el.Name]
		}
		return false
	}

	// Least fixed point: a rule is nullable if any alternative is, and
	// that can only become true as more rules are found nullable.
	for changed := true; changed; {
		changed = false
		for _, p := range prods {
			if nullable[p.Name] {
				continue
			}
			for _, a := range p.Alts {
				if altNullable(a, elNullable) {
					nullable[p.Name] = true
					changed = true
					break
				}
			}
		}
	}

	count := func(alts []bnf.Sequence) int {
		n := 0
		for _, a := range alts {
			if altNullable(a, elNullable) {
				n++
			}
		}
		return n
	}
	complain := func(subject string, n int) error {
		return &ParseError{Message: fmt.Sprintf(
			"ebnf: %s has %d alternatives that each match nothing, so the "+
				"grammar is ambiguous: an empty input has more than one "+
				"derivation and no lookahead can choose between them. Make at "+
				"most one alternative optional — '(A | B)?' rather than "+
				"'A? | B?'", subject, n)}
	}

	// A group is a choice like any other, so it carries the same
	// ambiguity: `A ::= ("x"? | "y"?) "y"` mis-parses exactly as the
	// production-level shape does.
	var checkGroups func(el *bnf.Element, rule string) error
	checkGroups = func(el *bnf.Element, rule string) error {
		switch el.Kind {
		case bnf.KindGroup:
			if n := count(el.Alts); n > 1 {
				return complain(fmt.Sprintf("a group in rule '%s'", rule), n)
			}
			for _, a := range el.Alts {
				for _, e := range a {
					if err := checkGroups(e, rule); err != nil {
						return err
					}
				}
			}
		case bnf.KindOpt, bnf.KindStar, bnf.KindPlus, bnf.KindRep:
			return checkGroups(el.Inner, rule)
		}
		return nil
	}

	for _, p := range prods {
		if n := count(p.Alts); n > 1 {
			return complain(fmt.Sprintf("rule '%s'", p.Name), n)
		}
		for _, a := range p.Alts {
			for _, e := range a {
				if err := checkGroups(e, p.Name); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

func altNullable(a bnf.Sequence, elNullable func(*bnf.Element) bool) bool {
	for _, el := range a {
		if !elNullable(el) {
			return false
		}
	}
	return true
}
