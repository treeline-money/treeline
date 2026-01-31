import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'

// Disable macOS autocorrect/autocapitalize on all text inputs
function disableAutocorrect(el: Element) {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const type = el.type?.toLowerCase();
    if (type !== 'checkbox' && type !== 'radio' && type !== 'submit' && type !== 'button') {
      el.setAttribute('autocomplete', 'off');
      el.setAttribute('autocorrect', 'off');
      el.setAttribute('autocapitalize', 'off');
      el.setAttribute('spellcheck', 'false');
    }
  }
}

// Apply to existing and future inputs
const observer = new MutationObserver((mutations) => {
  mutations.forEach((mutation) => {
    mutation.addedNodes.forEach((node) => {
      if (node instanceof Element) {
        disableAutocorrect(node);
        node.querySelectorAll('input, textarea').forEach(disableAutocorrect);
      }
    });
  });
});

observer.observe(document.body, { childList: true, subtree: true });

// Apply to any existing inputs
document.querySelectorAll('input, textarea').forEach(disableAutocorrect);

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
