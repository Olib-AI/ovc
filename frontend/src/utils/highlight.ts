import hljs from 'highlight.js/lib/core';
import langBash from 'highlight.js/lib/languages/bash';
import langC from 'highlight.js/lib/languages/c';
import langCpp from 'highlight.js/lib/languages/cpp';
import langCss from 'highlight.js/lib/languages/css';
import langDiff from 'highlight.js/lib/languages/diff';
import langDockerfile from 'highlight.js/lib/languages/dockerfile';
import langGo from 'highlight.js/lib/languages/go';
import langGraphql from 'highlight.js/lib/languages/graphql';
import langIni from 'highlight.js/lib/languages/ini';
import langJava from 'highlight.js/lib/languages/java';
import langJs from 'highlight.js/lib/languages/javascript';
import langJson from 'highlight.js/lib/languages/json';
import langKotlin from 'highlight.js/lib/languages/kotlin';
import langMarkdown from 'highlight.js/lib/languages/markdown';
import langPhp from 'highlight.js/lib/languages/php';
import langPython from 'highlight.js/lib/languages/python';
import langRuby from 'highlight.js/lib/languages/ruby';
import langRust from 'highlight.js/lib/languages/rust';
import langScala from 'highlight.js/lib/languages/scala';
import langShell from 'highlight.js/lib/languages/shell';
import langSql from 'highlight.js/lib/languages/sql';
import langCsharp from 'highlight.js/lib/languages/csharp';
import langSwift from 'highlight.js/lib/languages/swift';
import langToml from 'highlight.js/lib/languages/ini'; // TOML shares ini grammar
import langTs from 'highlight.js/lib/languages/typescript';
import langXml from 'highlight.js/lib/languages/xml';
import langYaml from 'highlight.js/lib/languages/yaml';

hljs.registerLanguage('bash', langBash);
hljs.registerLanguage('c', langC);
hljs.registerLanguage('cpp', langCpp);
hljs.registerLanguage('csharp', langCsharp);
hljs.registerLanguage('css', langCss);
hljs.registerLanguage('diff', langDiff);
hljs.registerLanguage('dockerfile', langDockerfile);
hljs.registerLanguage('go', langGo);
hljs.registerLanguage('graphql', langGraphql);
hljs.registerLanguage('ini', langIni);
hljs.registerLanguage('java', langJava);
hljs.registerLanguage('javascript', langJs);
hljs.registerLanguage('json', langJson);
hljs.registerLanguage('kotlin', langKotlin);
hljs.registerLanguage('markdown', langMarkdown);
hljs.registerLanguage('php', langPhp);
hljs.registerLanguage('python', langPython);
hljs.registerLanguage('ruby', langRuby);
hljs.registerLanguage('rust', langRust);
hljs.registerLanguage('scala', langScala);
hljs.registerLanguage('shell', langShell);
hljs.registerLanguage('sql', langSql);
hljs.registerLanguage('swift', langSwift);
hljs.registerLanguage('toml', langToml);
hljs.registerLanguage('typescript', langTs);
hljs.registerLanguage('xml', langXml);
hljs.registerLanguage('yaml', langYaml);

// Map file extensions to highlight.js language identifiers
const EXT_TO_LANGUAGE: Record<string, string> = {
  '.bash': 'bash',
  '.c': 'c',
  '.cc': 'cpp',
  '.cpp': 'cpp',
  '.cs': 'csharp',
  '.css': 'css',
  '.cxx': 'cpp',
  '.diff': 'diff',
  '.dockerfile': 'dockerfile',
  '.go': 'go',
  '.graphql': 'graphql',
  '.gql': 'graphql',
  '.h': 'c',
  '.hpp': 'cpp',
  '.htm': 'xml',
  '.html': 'xml',
  '.ini': 'ini',
  '.java': 'java',
  '.js': 'javascript',
  '.json': 'json',
  '.jsx': 'javascript',
  '.kt': 'kotlin',
  '.kts': 'kotlin',
  '.md': 'markdown',
  '.markdown': 'markdown',
  '.patch': 'diff',
  '.php': 'php',
  '.py': 'python',
  '.rb': 'ruby',
  '.rs': 'rust',
  '.scala': 'scala',
  '.sh': 'bash',
  '.sql': 'sql',
  '.svg': 'xml',
  '.swift': 'swift',
  '.toml': 'toml',
  '.ts': 'typescript',
  '.tsx': 'typescript',
  '.xml': 'xml',
  '.yaml': 'yaml',
  '.yml': 'yaml',
  '.zsh': 'bash',
};

// Special filenames with no extension
const FILENAME_TO_LANGUAGE: Record<string, string> = {
  dockerfile: 'dockerfile',
  'Dockerfile': 'dockerfile',
  '.bashrc': 'bash',
  '.zshrc': 'bash',
  '.gitignore': 'bash',
  'Makefile': 'bash',
  'makefile': 'bash',
};

export function getExtension(path: string): string {
  const dot = path.lastIndexOf('.');
  return dot >= 0 ? path.slice(dot).toLowerCase() : '';
}

export function detectLanguage(filePath: string): string | null {
  const basename = filePath.split('/').pop() ?? filePath;
  if (FILENAME_TO_LANGUAGE[basename]) {
    return FILENAME_TO_LANGUAGE[basename];
  }
  const ext = getExtension(filePath);
  return EXT_TO_LANGUAGE[ext] ?? null;
}

/**
 * Highlight a full source text and return per-line HTML strings.
 * highlight.js operates on the complete source so multi-line constructs
 * (block comments, template literals) are tokenised correctly.  We then
 * split the resulting HTML on newline boundaries — this is safe because
 * hljs never emits bare newlines inside an open span tag.
 */
export function highlightLines(code: string, language: string): string[] | null {
  try {
    const result = hljs.highlight(code, { language, ignoreIllegals: true });
    return result.value.split('\n');
  } catch {
    return null;
  }
}

export { hljs };
