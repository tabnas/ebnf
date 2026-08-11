/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

/*  ebnf.ts
 *  EBNF plugin — adds `tn.ebnf(src)` (install) and `tn.ebnf.toSpec(src)`
 *  (build without installing) to a Tabnas instance.
 *
 *  The conversion logic itself lives in ./converter.ts; this file
 *  exposes it both as a Plugin (for `tn.use(ebnf)`) and as bare
 *  exports (for code that wants to convert without an instance).
 */

import type {
  Tabnas,
  GrammarSpec,
  Plugin,
} from '@tabnas/parser'

import {
  ebnf as ebnfConvert,
  parseEbnf,
  emitGrammarSpec,
  eliminateLeftRecursion,
  ebnfRules,
  EbnfParseError,
  EbnfCompileError,
  EbnfConvertOptions,
  EbnfElement,
  EbnfSequence,
  EbnfProduction,
  EbnfGrammar,
} from './converter'


// Plugin entry point. Decorates the instance with a callable `ebnf`
// member that converts and installs a grammar, plus `ebnf.toSpec` for
// callers that just want the spec.
const ebnf: Plugin = function ebnf(tn: Tabnas, _options?: any): void {
  const fn = ((src: string, opts?: EbnfConvertOptions): GrammarSpec => {
    const spec = ebnfConvert(src, opts)
    tn.grammar(spec)
    return spec
  }) as ((src: string, opts?: EbnfConvertOptions) => GrammarSpec) & {
    toSpec: (src: string, opts?: EbnfConvertOptions) => GrammarSpec
  }
  fn.toSpec = (src: string, opts?: EbnfConvertOptions): GrammarSpec =>
    ebnfConvert(src, opts)
  tn.ebnf = fn
}


// `toSpec` as a bare export, for callers with no instance to decorate.
const toSpec = (src: string, opts?: EbnfConvertOptions): GrammarSpec =>
  ebnfConvert(src, opts)


export {
  VERSION,
  ebnf,
  ebnfConvert,
  toSpec,
  parseEbnf,
  emitGrammarSpec,
  eliminateLeftRecursion,
  ebnfRules,
  EbnfParseError,
  EbnfCompileError,
}

// The IR aliases go out through the facade too. Without them a caller
// cannot name the type of what `parseEbnf` returns without importing
// `@tabnas/bnf` directly — which is precisely what the aliases exist to
// avoid.
export type {
  EbnfConvertOptions,
  EbnfElement,
  EbnfSequence,
  EbnfProduction,
  EbnfGrammar,
}


// VERSION is this package's version. It MUST equal package.json
// "version": the release orchestrator rewrites both, and the version
// test fails the build if they drift.
const VERSION = '0.1.2'
