/** コミットのメッセージの編集欄（History の近道と編集モード）。保存は Ctrl/Cmd+Enter、取り消しは Esc */
export function openMessageEditor(title: string, message: string, onSave: (text: string) => void, onClose?: () => void): void {
  const overlay = document.createElement('div');
  overlay.className = 'overlay';
  const box = document.createElement('div');
  box.className = 'editor';
  const heading = document.createElement('div');
  heading.className = 'heading';
  heading.textContent = title;
  const textarea = document.createElement('textarea');
  textarea.value = message;
  textarea.spellcheck = false;
  const buttons = document.createElement('div');
  buttons.className = 'buttons';
  const save = document.createElement('button');
  save.textContent = 'Save';
  const cancel = document.createElement('button');
  cancel.textContent = 'Cancel';
  cancel.className = 'secondary';
  buttons.append(cancel, save);
  box.append(heading, textarea, buttons);
  overlay.append(box);
  document.body.append(overlay);
  textarea.focus();
  textarea.setSelectionRange(0, textarea.value.indexOf('\n') < 0 ? textarea.value.length : textarea.value.indexOf('\n'));

  const close = () => {
    overlay.remove();
    onClose?.();
  };
  const submit = () => {
    if (!textarea.value.trim()) {
      textarea.focus();
      return;
    }
    onSave(textarea.value);
    close();
  };
  save.addEventListener('click', submit);
  cancel.addEventListener('click', close);
  textarea.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  });
}
