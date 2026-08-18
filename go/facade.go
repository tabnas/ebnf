// Copyright (c) 2026 Richard Rodger and other contributors, MIT License

// facade.go — the package's public surface, mirroring ts/src/ebnf.ts.
package ebnf

import (
	"strings"

	bnf "github.com/tabnas/bnf/go"
	tabnas "github.com/tabnas/parser/go"
)

// The IR types under this package's own names, so a caller can talk
// about what ParseEbnf returns without importing @tabnas/bnf directly.
// Aliases, not definitions: a value has to cross the package boundary.
type (
	EbnfElement    = bnf.Element
	EbnfSequence   = bnf.Sequence
	EbnfProduction = bnf.Production
	EbnfGrammar    = bnf.Grammar
	ConvertOptions = bnf.ConvertOptions
)

// ParseError is raised when the EBNF source itself cannot be read: a
// syntax error, or a construct this front-end deliberately refuses.
// Line and Column locate the offending token where one is available.
type ParseError struct {
	Message string
	Line    int
	Column  int
	Cause   error
}

func (e *ParseError) Error() string { return e.Message }
func (e *ParseError) Unwrap() error { return e.Cause }

// CompileError is raised when the EBNF parsed cleanly but the shared
// compiler could not compile the resulting IR — an unknown rule
// reference, a purely left-recursive rule, an ambiguous FIRST set. The
// compiler's message is kept verbatim; only its package prefix is
// restamped, so a user who wrote EBNF never sees the name of a package
// they did not import.
type CompileError struct {
	Message string
	Cause   error
}

func (e *CompileError) Error() string { return e.Message }
func (e *CompileError) Unwrap() error { return e.Cause }

// Ebnf converts EBNF source into a tabnas grammar spec: parse this
// notation, then hand the IR to the shared compiler.
func Ebnf(src string, opts *ConvertOptions) (*tabnas.GrammarSpec, error) {
	grammar, err := ParseEbnf(src)
	if err != nil {
		return nil, err
	}
	if opts == nil {
		opts = &ConvertOptions{}
	}
	if opts.Tag == "" {
		clone := *opts
		clone.Tag = "ebnf"
		opts = &clone
	}
	spec, err := emitSafely(grammar, opts)
	if err != nil {
		return nil, err
	}
	return spec, nil
}

// emitSafely runs the shared compiler and turns its failure into an
// EBNF-flavoured error.
//
// The recover is not decoration: the Go compiler pipeline PANICS on a
// grammar it cannot compile, where the TypeScript one throws a
// catchable error. That is the wrong contract for a library — invalid
// user input should be an error return — but it is inherited, and the
// ABNF front-end's suite pins it, so it is contained here rather than
// changed underneath another package.
func emitSafely(
	g *bnf.Grammar, opts *ConvertOptions) (spec *tabnas.GrammarSpec, err error) {
	defer func() {
		if r := recover(); r != nil {
			msg := ""
			// The panicked value is kept as the cause, not just its
			// text: the compiler raises the purely-left-recursive
			// rejection as a *bnf.EmitError so the SPAN survives the
			// panic, and stringifying here would discard it at the
			// last step. With the cause attached, `errors.As` recovers
			// the range on this path exactly as on the return path
			// below.
			var cause error
			switch v := r.(type) {
			case error:
				msg, cause = v.Error(), v
			case string:
				msg = v
			default:
				panic(r)
			}
			err = &CompileError{Message: restamp(msg), Cause: cause}
		}
	}()
	spec, err = bnf.EmitGrammarSpec(g, opts)
	if err != nil {
		return nil, &CompileError{Message: restamp(err.Error()), Cause: err}
	}
	return spec, nil
}

// restamp replaces the shared compiler's package prefix with this
// front-end's, leaving the rest of the message — which names the
// offending rule — exactly as the compiler wrote it.
func restamp(msg string) string {
	for _, p := range []string{"bnf: ", "abnf: "} {
		if strings.HasPrefix(msg, p) {
			return "ebnf: " + msg[len(p):]
		}
	}
	return msg
}

// ToSpec is Ebnf under the name the TS package uses for the bare
// conversion entry point.
func ToSpec(src string, opts *ConvertOptions) (*tabnas.GrammarSpec, error) {
	return Ebnf(src, opts)
}

// EliminateLeftRecursion exposes that pass for a caller that wants to
// inspect the rewritten IR.
func EliminateLeftRecursion(g *EbnfGrammar) *EbnfGrammar {
	return bnf.EliminateLeftRecursion(g)
}

// Install converts EBNF source and installs it on a tabnas instance.
// Use a fresh instance per grammar: installing applies lexer settings as
// well as rules, and those are instance-wide.
func Install(
	j *tabnas.Tabnas, src string, opts *ConvertOptions,
) (*tabnas.GrammarSpec, error) {
	spec, err := Ebnf(src, opts)
	if err != nil {
		return nil, err
	}
	if err := j.Grammar(spec); err != nil {
		return nil, err
	}
	return spec, nil
}
