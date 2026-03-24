import { useEffect } from 'react';

const DEFAULT_TITLE = 'OVC \u2014 Olib Version Control';

function useDocumentTitle(title: string) {
  useEffect(() => {
    document.title = title;
    return () => {
      document.title = DEFAULT_TITLE;
    };
  }, [title]);
}

export { useDocumentTitle };
