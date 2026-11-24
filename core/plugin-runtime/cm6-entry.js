import * as autocomplete from '@codemirror/autocomplete';
import * as commands from '@codemirror/commands';
import * as language from '@codemirror/language';
import * as lint from '@codemirror/lint';
import * as search from '@codemirror/search';
import * as state from '@codemirror/state';
import * as view from '@codemirror/view';
import * as markdown from '@codemirror/lang-markdown';
import * as common from '@lezer/common';
import * as highlight from '@lezer/highlight';
import * as lr from '@lezer/lr';
import * as lezerMarkdown from '@lezer/markdown';

// A compatibility view over the genuine Markdown tree, only for plugin iteration.
// Keep the real parser/tree intact. Obsidian names fenced/indented code blocks
// HyperMD-codeblock and exposes the *content* of inline code as a token node.
const tokenClassNodeProp = new common.NodeProp();
function syntaxTree(editorState) {
  const tree = language.syntaxTree(editorState);
  return new Proxy(tree, { get(target, key) {
    if (key !== 'iterate') { const value = Reflect.get(target, key); return typeof value === 'function' ? value.bind(target) : value; }
    return options => target.iterate({ ...options, enter(ref) {
      const code = ref.name === 'InlineCode';
      const block = ref.name === 'FencedCode' || ref.name === 'CodeBlock';
      if (!code && !block) return options.enter?.(ref);
      const raw = editorState.sliceDoc(ref.from, ref.to);
      const ticks = code ? /^`+/.exec(raw)?.[0].length || 1 : 0;
      const type = new Proxy(ref.type, { get(type, prop) {
        if (prop === 'prop') return name => name === tokenClassNodeProp ? (code ? 'inline-code' : 'codeblock') : type.prop(name);
        return Reflect.get(type, prop);
      } });
      const node = { from: ref.from + ticks, to: ref.to - ticks, name: block ? 'HyperMD-codeblock' : ref.name, type };
      node.node = node;
      options.enter?.(node);
      return false;
    }, leave: options.leave });
  } });
}
const contextEffect = state.StateEffect.define();
const editorInfoField = state.StateField.define({ create: () => ({}), update(value, tr) {
  for (const effect of tr.effects) if (effect.is(contextEffect)) return effect.value.info;
  return value;
} });
const editorLivePreviewField = state.StateField.define({ create: () => false, update(value, tr) {
  for (const effect of tr.effects) if (effect.is(contextEffect)) return effect.value.live;
  return value;
} });
globalThis.PaperloomCM6 = { versions: PAPERLOOM_CM_VERSIONS, contextEffect, editorInfoField, editorLivePreviewField, markdown,
  modules: { '@codemirror/autocomplete': autocomplete, '@codemirror/commands': commands,
    '@codemirror/language': { ...language, syntaxTree, tokenClassNodeProp }, '@codemirror/lint': lint,
    '@codemirror/search': search, '@codemirror/state': state, '@codemirror/view': view,
    '@codemirror/lang-markdown': markdown, '@lezer/common': common, '@lezer/highlight': highlight, '@lezer/lr': lr, '@lezer/markdown': lezerMarkdown } };
