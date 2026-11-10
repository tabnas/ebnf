// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

package ebnf

// tree.go — an EBNF document read as the schema grammar-rules: its rules
// as the text writes them, uncompiled and in source order (tabnas/bnf's
// docs/grammar-rules.md, and section 13.1 of alchemy's
// docs/streaming-plan.md). The Go port of ts/src/tree.ts: the tree a host
// translates, whose events the part, alchemy/transduce.alc, makes from the
// rule passes of Make().
//
// The part reads a tabnas parser's rule passes, which every runtime must
// make alike, so the tree parser is the canonical meta-grammar as a tabnas
// rule table: ebnfRules in ts/src/converter.ts, its lexer and every
// alternate as they are, the table carried here as the grammar document
// rs/src/parser_ebnf.rs installs (metaGrammarText, which tree_test.go holds
// to that file), with actions of its own (treeRef below). The IR this
// port's compiler reads is still the hand-written scanner's
// (parser_ebnf.go); the tree parser is the first rule table here. A token
// whose text needs reading (a class, a code point) is given what it reads
// as its value by the action that reads it, since a part reads a token's
// value and never the work an action does with it. Objects are
// *tabnas.OrderedMap, so that their members keep the schema's order;
// numbers are float64.
//
// The tree holds what the schema allows, and EBNF what this package reads,
// so the parser refuses the rest, each with a code of its own: a name that
// is not one (ebnf_name), an empty string (ebnf_empty_literal), a code
// point past #x10FFFF or with no digits (ebnf_code_point), a class range
// that ends below where it starts (ebnf_class_range), an empty class
// (ebnf_empty_class), a stray bracket (ebnf_char_class), an empty
// alternative (ebnf_empty_alternative), a leading comma
// (ebnf_leading_comma), a subtraction (ebnf_subtraction), a special
// sequence or a ? where an element is due (ebnf_special_sequence), ISO
// bracket repetition (ebnf_iso_repetition), a * or + where an element is
// due (ebnf_dangling_postfix) and a group left open (ebnf_unclosed).

import (
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"

	tabnas "github.com/tabnas/parser/go"
)

// treeOptions is the codes the tree parser refuses a source with, beside
// the engine's: the Error and Hint options Make gives the parser.
var treeOptions = tabnas.Options{
	Error: map[string]string{
		"ebnf_name":              "the name {src} is not a symbol name: one starts with a letter or an underscore, and goes on with letters, digits, _, . and -",
		"ebnf_empty_literal":     "the string {src} is empty, and EBNF has no epsilon terminal",
		"ebnf_code_point":        "the code point in {src} has no hex digits, or names no Unicode code point (the largest is #x10FFFF)",
		"ebnf_class_range":       "the character class {src} has a range that ends below where it starts",
		"ebnf_empty_class":       "the character class {src} is empty, and matches nothing",
		"ebnf_char_class":        "the {src} opens or closes no character class: a class opens and closes on one line, and holds no ]",
		"ebnf_empty_alternative": "an alternative is empty, and EBNF has no epsilon terminal",
		"ebnf_leading_comma":     "the comma separates nothing: ISO 14977 writes it between two items",
		"ebnf_subtraction":       "the subtraction {src} is not supported: the tree has no difference of two languages",
		"ebnf_special_sequence":  "the {src} is where an element is due: a postfix ? follows an element, and ISO 14977 special sequences are not supported",
		"ebnf_iso_repetition":    "the bracket {src} is ISO 14977 repetition, which this dialect does not read",
		"ebnf_dangling_postfix":  "the {src} is where an element is due: repetition in this dialect is postfix",
		"ebnf_unclosed":          "a group is not closed",
	},
	Hint: map[string]string{
		"ebnf_name":              "Spell a name as a letter or an underscore, then letters, digits, _, . and -.",
		"ebnf_empty_literal":     "Write A? to make A optional.",
		"ebnf_code_point":        "A code point is #x and its hex digits, no larger than #x10FFFF.",
		"ebnf_class_range":       "Write a range from its lower end to its higher, as [a-z].",
		"ebnf_empty_class":       "A class holds a character or a range at least.",
		"ebnf_char_class":        `Write a literal ] as the string "]", and the ISO option [ A ] as A?.`,
		"ebnf_empty_alternative": "Write A? rather than A |.",
		"ebnf_leading_comma":     "Write A , B, with an item on each side of the comma.",
		"ebnf_subtraction":       "Write a difference of single characters as a negated class, as [^abc].",
		"ebnf_special_sequence":  "Write A? after the element it repeats.",
		"ebnf_iso_repetition":    "Write A* for { A }, and A? for the ISO [ A ].",
		"ebnf_dangling_postfix":  "Write A* and A+, after the element they repeat.",
		"ebnf_unclosed":          "Close the group with ).",
	},
}

// metaGrammarText is the EBNF meta-grammar's rule table, the grammar
// document rs/src/parser_ebnf.rs installs as GRAMMAR_TEXT: the canonical
// ebnfRules, each alternate's s, p, r, b, g, and its action and condition
// named for the reference treeRef binds.
//
//	ebnf ::= prod*
//	prod ::= NAME ('::=' | '=') alts ';'?
//	alts ::= seq ('|' seq)*
//	seq  ::= (','? item)*
//	item ::= atom post
//	post ::= ('?' | '*' | '+')*
//	atom ::= NAME | STRING | CHARCLASS | HEX | '(' alts ')'
const metaGrammarText = `
{
  "rule": {
    "ebnf": {
      "open": [
        { "s": "#ZZ", "g": "empty" },
        { "p": "prod" }
      ],
      "close": [
        { "s": "#ZZ" }
      ]
    },

    "prod": {
      "open": [
        { "s": "#TX #DEFOP", "a": "@prod-name", "p": "alts" }
      ],
      "close": [
        { "s": "#SC #TX #DEFOP", "b": 2, "r": "prod" },
        { "s": "#SC" },
        { "s": "#TX #DEFOP", "b": 2, "r": "prod" },
        { "b": 1 }
      ]
    },

    "alts": {
      "open": [
        { "p": "seq" }
      ],
      "close": [
        { "s": "#ALT", "p": "seq" },
        { "b": 1 }
      ]
    },

    "seq": {
      "open": [
        { "s": "#TX #DEFOP", "b": 2, "a": "@fail-empty-alt" },
        { "s": "#ALT", "a": "@fail-empty-alt" },
        { "s": "#RP", "a": "@fail-empty-alt" },
        { "s": "#SC", "a": "@fail-empty-alt" },
        { "s": "#ZZ", "a": "@fail-empty-alt" },
        { "s": "#CA", "a": "@fail-leading-comma" },

        { "s": "#ST", "b": 1, "p": "item" },
        { "s": "#CC", "b": 1, "p": "item" },
        { "s": "#HX", "b": 1, "p": "item" },
        { "s": "#TX", "b": 1, "p": "item" },
        { "s": "#LP", "b": 1, "p": "item" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" },
        { "s": "#CS", "a": "@fail-char-class" },
        { "s": "#STAR", "a": "@fail-dangling-postfix" },
        { "s": "#PLUS", "a": "@fail-dangling-postfix" },

        { "b": 1 }
      ],
      "close": [
        { "s": "#TX #DEFOP", "b": 2, "g": "end" },
        { "s": "#ALT", "b": 1, "g": "end" },
        { "s": "#RP", "b": 1, "g": "end" },
        { "s": "#SC", "b": 1, "g": "end" },
        { "s": "#ZZ", "b": 1, "g": "end" },
        { "s": "#CA", "p": "item" },

        { "s": "#ST", "b": 1, "p": "item" },
        { "s": "#CC", "b": 1, "p": "item" },
        { "s": "#HX", "b": 1, "p": "item" },
        { "s": "#TX", "b": 1, "p": "item" },
        { "s": "#LP", "b": 1, "p": "item" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" },
        { "s": "#CS", "a": "@fail-char-class" },
        { "s": "#STAR", "a": "@fail-dangling-postfix" },
        { "s": "#PLUS", "a": "@fail-dangling-postfix" },

        { "b": 1 }
      ]
    },

    "item": {
      "open": [
        { "p": "atom" }
      ],
      "close": [
        { "c": "@item-first", "a": "@item-take", "p": "post" },
        { "b": 1 }
      ]
    },

    "post": {
      "open": [
        { "s": "#QM", "a": "@post-opt", "p": "post" },
        { "s": "#STAR", "a": "@post-star", "p": "post" },
        { "s": "#PLUS", "a": "@post-plus", "p": "post" },
        { "b": 1, "g": "empty" }
      ],
      "close": [
        { "b": 1 }
      ]
    },

    "atom": {
      "open": [
        { "s": "#ST", "a": "@atom-st" },
        { "s": "#CC", "a": "@atom-cc" },
        { "s": "#HX", "a": "@atom-hx" },
        { "s": "#TX", "a": "@atom-tx" },
        { "s": "#LP", "a": "@atom-lp", "p": "alts" },

        { "s": "#QM", "a": "@fail-special-sequence" },
        { "s": "#SUB", "a": "@fail-subtraction" },
        { "s": "#OB", "a": "@fail-iso-repetition" },
        { "s": "#CB", "a": "@fail-iso-repetition" },
        { "s": "#OS", "a": "@fail-char-class" }
      ],
      "close": [
        { "s": "#RP", "c": "@atom-group-c", "a": "@atom-group-close" },
        { "c": "@atom-group-c", "a": "@fail-unclosed-group" },
        { "b": 1 }
      ]
    }
  }
}
`

// metaOptions is the engine options the meta-grammar reads its source
// with: ebnfOptions in ts/src/converter.ts.
func metaOptions() tabnas.Options {
	yes, no := true, false
	str := func(s string) *string { return &s }
	return tabnas.Options{
		Rule: &tabnas.RuleOptions{Start: "ebnf"},
		Fixed: &tabnas.FixedOptions{Token: map[string]*string{
			// `:` is not an operator on its own (`::=` is matched whole),
			// so colons stay free to appear inside names.
			"#CL": nil,
			// `[` `]` `{` `}` stay declared so the text matcher treats
			// them as delimiters; a well-formed class is claimed by #CC
			// first, so these only ever surface on malformed input.
			"#OS": str("["),
			"#CS": str("]"),
			"#OB": str("{"),
			"#CB": str("}"),
			// ISO 14977 concatenation and termination, read and dropped.
			"#CA":   str(","),
			"#SC":   str(";"),
			"#ALT":  str("|"),
			"#STAR": str("*"),
			"#PLUS": str("+"),
			"#QM":   str("?"),
			"#LP":   str("("),
			"#RP":   str(")"),
		}},
		Match: &tabnas.MatchOptions{
			// Every match token is eager: each starts with a character that
			// has one meaning in EBNF (see eager in ts/src/converter.ts).
			Token: map[string]*regexp.Regexp{
				"#DEF":  regexp.MustCompile(`^::=`),
				"#DEFE": regexp.MustCompile(`^=`),
				// A complete class, bounded to one line so an unclosed `[`
				// fails at the bracket.
				"#CC": regexp.MustCompile(`^\[\^?[^\]\n]*\]`),
				// A standalone code point; the x is lowercase, as W3C writes.
				"#HX":  regexp.MustCompile(`^#x[0-9a-fA-F]+`),
				"#SUB": regexp.MustCompile(`^-`),
			},
			TokenEager: map[string]bool{"#DEF": true, "#DEFE": true, "#CC": true, "#HX": true, "#SUB": true},
			TokenOrder: []string{"#DEF", "#DEFE", "#CC", "#HX", "#SUB"},
			// ISO comments, `(* ... *)`, claimed before the fixed matcher
			// sees `(`.
			Check: isoComment,
		},
		// `NAME ::=` and `NAME =` share one lookahead pattern.
		TokenSet: map[string][]string{"DEFOP": {"#DEF", "#DEFE"}},
		// A W3C symbol name is an ordinary word: true, false and null are
		// names, never value tokens.
		Value: &tabnas.ValueOptions{Lex: &no},
		// EBNF has no numeric literals.
		Number: &tabnas.NumberOptions{Lex: &no},
		// W3C EBNF quotes are ' and ", and a literal has no escapes: the
		// escape character is DEL, which no EBNF literal holds.
		String: &tabnas.StringOptions{Chars: `'"`, EscapeChar: "\x7F"},
		Comment: &tabnas.CommentOptions{Def: map[string]*tabnas.CommentDef{
			// `#` starts a code point, and `//` is no EBNF comment.
			"hash":  nil,
			"slash": nil,
			"multi": {Line: &no, Start: "/*", End: "*/", Lex: &yes, EatLine: &no},
		}},
	}
}

// isoComment claims an ISO 14977 comment, `(* ... *)`, as a #CM the parser
// skips, counting the lines it spans: isoCommentMatcher in
// ts/src/converter.ts, which is an eager match token there. A function
// matcher is never eager in this engine, so it runs as the match
// matcher's check, before every match token, which is where an eager one
// is tried where no rule expects it. An unterminated `(*` is left to `(`
// and `*`.
func isoComment(lex *tabnas.Lex) *tabnas.LexCheckResult {
	pnt := lex.Cursor()
	rest := lex.Src[pnt.SI:]
	if !strings.HasPrefix(rest, "(*") {
		return nil
	}
	end := strings.Index(rest[2:], "*)")
	if end < 0 {
		return nil
	}
	text := rest[:end+4]
	tkn := lex.Token("#CM", tabnas.TinCM, text, text)
	for _, c := range text {
		if '\n' == c {
			pnt.RI++
			pnt.CI = 1
		} else {
			pnt.CI++
		}
	}
	pnt.SI += len(text)
	return &tabnas.LexCheckResult{Done: true, Token: tkn}
}

// treeObject is an object of the tree, its members in the order given.
func treeObject(kv ...any) *tabnas.OrderedMap {
	m := tabnas.NewOrderedMap()
	for i := 0; i+1 < len(kv); i += 2 {
		m.Set(kv[i].(string), kv[i+1])
	}
	return m
}

// treeList is a list a rule builds, held by pointer so that a child rule,
// which inherits the node, appends to its parent's.
type treeList = []any

// listOf reads the list a child rule built.
func listOf(r *tabnas.Rule) (treeList, bool) {
	if r == nil || r == tabnas.NoRule {
		return nil, false
	}
	p, ok := r.Node.(*treeList)
	if !ok {
		return nil, false
	}
	return append(treeList{}, (*p)...), true
}

// codePointOf is the code point a run of hex digits names, or -1 where it
// names none.
func codePointOf(hex string) int {
	n, err := strconv.ParseUint(hex, 16, 64)
	if err != nil || 0x10FFFF < n {
		return -1
	}
	return int(n)
}

func isHex(c byte) bool {
	return '0' <= c && c <= '9' || 'a' <= c && c <= 'f' || 'A' <= c && c <= 'F'
}

// classValue reads a class's members as the IR parser reads them: `^`
// first negates it, `#xNN` is a code point, `a-b` is a range, and any
// other `-` is a member of its own. It answers the class's value, or the
// code the class is refused with.
func classValue(src string) (*tabnas.OrderedMap, string) {
	negated := 2 < len(src) && '^' == src[1]
	body := src[1 : len(src)-1]
	if negated {
		body = src[2 : len(src)-1]
	}
	if "" == body {
		return nil, "ebnf_empty_class"
	}
	// A code point, or -1 for a `-`.
	var items []int
	for i := 0; i < len(body); {
		c := body[i]
		if '#' == c && i+1 < len(body) && 'x' == body[i+1] {
			j := i + 2
			for j < len(body) && isHex(body[j]) {
				j++
			}
			cp := -1
			if j != i+2 {
				cp = codePointOf(body[i+2 : j])
			}
			if cp < 0 {
				return nil, "ebnf_code_point"
			}
			items = append(items, cp)
			i = j
			continue
		}
		if '-' == c {
			items = append(items, -1)
			i++
			continue
		}
		r, size := utf8.DecodeRuneInString(body[i:])
		items = append(items, int(r))
		i += size
	}
	var members []any
	for k := 0; k < len(items); k++ {
		it := items[k]
		if -1 == it {
			members = append(members, treeObject("from", float64(0x2d), "to", float64(0x2d)))
			continue
		}
		if k+2 < len(items) && -1 == items[k+1] && -1 != items[k+2] {
			hi := items[k+2]
			if hi < it {
				return nil, "ebnf_class_range"
			}
			members = append(members, treeObject("from", float64(it), "to", float64(hi)))
			k += 2
			continue
		}
		members = append(members, treeObject("from", float64(it), "to", float64(it)))
	}
	return treeObject("negated", negated, "members", members), ""
}

// copyMembers is a class's members, each object a copy of its own.
func copyMembers(list []any) []any {
	out := make([]any, len(list))
	for i, m := range list {
		om := m.(*tabnas.OrderedMap)
		from, _ := om.Get("from")
		to, _ := om.Get("to")
		out[i] = treeObject("from", from, "to", to)
	}
	return out
}

// tokText is a token's value as text: a string's characters, a name's
// spelling.
func tokText(t *tabnas.Token, r *tabnas.Rule, ctx *tabnas.Context) string {
	if s, ok := t.ResolveVal(r, ctx).(string); ok {
		return s
	}
	return t.Src
}

// refuse raises a code on the first token an open state matched, and
// refuseClose on the first a close state matched, as ts/src/tree.ts's
// refuse and refuseClose do.
func refuse(code string) tabnas.AltAction {
	return func(r *tabnas.Rule, ctx *tabnas.Context) {
		ctx.ParseErr = r.O[0].Bad(code)
	}
}

func refuseClose(code string) tabnas.AltAction {
	return func(r *tabnas.Rule, ctx *tabnas.Context) {
		ctx.ParseErr = r.C[0].Bad(code)
	}
}

// inClose is whether a rule's action runs in its close state, where the
// tokens it matched are its close state's.
func inClose(r *tabnas.Rule) bool {
	return tabnas.CLOSE == r.State
}

// refuseEither raises a code on the first token the state the alternate
// belongs to matched: the meta-grammar names one action for an alternate
// seq's open and close states share.
func refuseEither(code string) tabnas.AltAction {
	return func(r *tabnas.Rule, ctx *tabnas.Context) {
		if inClose(r) {
			refuseClose(code)(r, ctx)
			return
		}
		refuse(code)(r, ctx)
	}
}

// pushRepeat appends a repetition to the run a `post` rule builds.
func pushRepeat(rep *tabnas.OrderedMap) tabnas.AltAction {
	return func(r *tabnas.Rule, _ *tabnas.Context) {
		if p, ok := r.Node.(*treeList); ok {
			*p = append(*p, rep)
		}
	}
}

// treeRef is the tree parser's actions, under the names metaGrammarText
// gives them: what ts/src/tree.ts's TREE builds.
func treeRef() map[tabnas.FuncRef]any {
	return map[tabnas.FuncRef]any{
		"@ebnf-bo": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.Node = treeObject("type", "grammar", "rules", []any{})
		}),

		"@prod-name": tabnas.AltAction(func(r *tabnas.Rule, ctx *tabnas.Context) {
			name := tokText(r.O[0], r, ctx)
			if !validName(name) {
				ctx.ParseErr = r.O[0].Bad("ebnf_name")
				return
			}
			r.EnsureU()["name"] = name
		}),
		"@prod-bc": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			alts, ok := listOf(r.Child)
			if !ok {
				return
			}
			rule := treeObject("type", "rule", "name", r.EnsureU()["name"], "alternatives", alts)
			if g, ok := r.Node.(*tabnas.OrderedMap); ok {
				rules, _ := g.Get("rules")
				list, _ := rules.([]any)
				g.Set("rules", append(list, rule))
			}
		}),

		"@alts-bo": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.Node = &treeList{}
		}),
		"@alts-bc": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			seq, ok := listOf(r.Child)
			if !ok {
				return
			}
			if p, ok := r.Node.(*treeList); ok {
				*p = append(*p, seq)
			}
		}),

		"@seq-bo": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.Node = &treeList{}
		}),
		"@fail-empty-alt":     refuse("ebnf_empty_alternative"),
		"@fail-leading-comma": refuse("ebnf_leading_comma"),
		// An element's place: seq's open and close states, and atom's open.
		"@fail-special-sequence": refuseEither("ebnf_special_sequence"),
		"@fail-subtraction":      refuseEither("ebnf_subtraction"),
		"@fail-iso-repetition":   refuseEither("ebnf_iso_repetition"),
		"@fail-char-class":       refuseEither("ebnf_char_class"),
		"@fail-dangling-postfix": refuseEither("ebnf_dangling_postfix"),

		"@item-first": tabnas.AltCond(func(r *tabnas.Rule, _ *tabnas.Context) bool {
			got, _ := r.EnsureU()["got"].(bool)
			return !got
		}),
		"@item-take": tabnas.AltAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.EnsureU()["got"] = true
			if r.Child != nil && r.Child != tabnas.NoRule {
				r.EnsureU()["atom"] = r.Child.Node
			}
		}),
		// The atom, its repetitions appended, onto the alternative.
		"@item-bc": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			got, _ := r.EnsureU()["got"].(bool)
			el, ok := r.EnsureU()["atom"].(*tabnas.OrderedMap)
			if !got || !ok {
				return
			}
			if reps, ok := listOf(r.Child); ok && 0 < len(reps) {
				copied := make([]any, len(reps))
				for i, rep := range reps {
					om := rep.(*tabnas.OrderedMap)
					c := tabnas.NewOrderedMap()
					for _, k := range om.Keys {
						c.Set(k, om.Vals[k])
					}
					copied[i] = c
				}
				el.Set("repeat", copied)
			}
			if p, ok := r.Node.(*treeList); ok {
				*p = append(*p, el)
			}
		}),

		// The postfix operators, innermost first, each a repetition.
		"@post-bo": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.Node = &treeList{}
		}),
		"@post-opt":  pushRepeat(treeObject("min", float64(0), "max", float64(1))),
		"@post-star": pushRepeat(treeObject("min", float64(0))),
		"@post-plus": pushRepeat(treeObject("min", float64(1))),
		"@post-bc": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			inner, ok := listOf(r.Child)
			if !ok {
				return
			}
			if p, ok := r.Node.(*treeList); ok {
				*p = append(*p, inner...)
			}
		}),

		"@atom-bo": tabnas.StateAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.Node = nil
			r.EnsureU()["group"] = false
		}),
		"@atom-st": tabnas.AltAction(func(r *tabnas.Rule, ctx *tabnas.Context) {
			text := tokText(r.O[0], r, ctx)
			if "" == text {
				ctx.ParseErr = r.O[0].Bad("ebnf_empty_literal")
				return
			}
			r.Node = treeObject("type", "literal", "text", text, "caseSensitive", true)
		}),
		// The class's negation and members, which its token carries from
		// here.
		"@atom-cc": tabnas.AltAction(func(r *tabnas.Rule, ctx *tabnas.Context) {
			read, code := classValue(r.O[0].Src)
			if "" != code {
				ctx.ParseErr = r.O[0].Bad(code)
				return
			}
			r.O[0].Val = read
			negated, _ := read.Get("negated")
			members, _ := read.Get("members")
			r.Node = treeObject("type", "class", "negated", negated,
				"members", copyMembers(members.([]any)))
		}),
		// The code point, which its token carries from here.
		"@atom-hx": tabnas.AltAction(func(r *tabnas.Rule, ctx *tabnas.Context) {
			value := codePointOf(r.O[0].Src[2:])
			if value < 0 {
				ctx.ParseErr = r.O[0].Bad("ebnf_code_point")
				return
			}
			r.O[0].Val = float64(value)
			r.Node = treeObject("type", "char", "value", float64(value))
		}),
		"@atom-tx": tabnas.AltAction(func(r *tabnas.Rule, ctx *tabnas.Context) {
			name := tokText(r.O[0], r, ctx)
			if !validName(name) {
				ctx.ParseErr = r.O[0].Bad("ebnf_name")
				return
			}
			r.Node = treeObject("type", "reference", "name", name)
		}),
		"@atom-lp": tabnas.AltAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			r.EnsureU()["group"] = true
		}),
		"@atom-group-c": tabnas.AltCond(func(r *tabnas.Rule, _ *tabnas.Context) bool {
			group, _ := r.EnsureU()["group"].(bool)
			return group
		}),
		"@atom-group-close": tabnas.AltAction(func(r *tabnas.Rule, _ *tabnas.Context) {
			alts, _ := listOf(r.Child)
			r.Node = treeObject("type", "group", "alternatives", alts)
		}),
		// A group the source ended inside, refused at its `(`.
		"@fail-unclosed-group": refuse("ebnf_unclosed"),
	}
}

// Make builds a fresh parser that reads an EBNF document as the tree: the
// parser a host streams a document through with the part, whose rule
// passes the part reads, and whose value is the tree, an
// *tabnas.OrderedMap (nil for a source that makes no pass).
func Make() *tabnas.Tabnas {
	opts := metaOptions()
	opts.Error = treeOptions.Error
	opts.Hint = treeOptions.Hint
	j := tabnas.Make(opts)
	// The engine ships no rules, and none may compete with the
	// meta-grammar's for the starting token set.
	for name := range j.RSM() {
		j.Rule(name, nil)
	}
	gs, err := tabnas.GrammarSpecFromJSON([]byte(metaGrammarText))
	if err == nil {
		gs.Ref = treeRef()
		err = j.Grammar(gs)
	}
	if err != nil {
		// The meta-grammar is this package's own, so it always installs.
		panic("ebnf: the tree parser does not install: " + err.Error())
	}
	return j
}

// ParseEbnfTree reads an EBNF document whole as the tree. A source the
// meta-grammar refuses, or the tree cannot hold, fails with a *ParseError
// naming the line and column, its Cause the engine's error with its code.
// The parser is built per call, so that two goroutines can read at once.
func ParseEbnfTree(src string) (*tabnas.OrderedMap, error) {
	out, err := Make().Parse(src)
	if err != nil {
		line, col := 0, 0
		if te, ok := err.(*tabnas.TabnasError); ok {
			line, col = te.Row, te.Col
		}
		loc := ""
		if line != 0 && col != 0 {
			loc = " at line " + strconv.Itoa(line) + ", column " + strconv.Itoa(col)
		}
		raw := strings.SplitN(err.Error(), "\n", 2)[0]
		return nil, &ParseError{
			Message: "ebnf: parse error" + loc + ": " + raw,
			Line:    line, Column: col, Cause: err,
		}
	}
	if tree, ok := out.(*tabnas.OrderedMap); ok {
		return tree, nil
	}
	// An empty source makes no rule pass, and the engine answers nothing
	// for it: it is the grammar of no rules, as a source of comments is.
	return treeObject("type", "grammar", "rules", []any{}), nil
}
