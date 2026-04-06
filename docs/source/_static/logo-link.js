document.addEventListener('DOMContentLoaded', function() {
  function addLogoLink() {
    const logos = document.querySelectorAll('.sidebar-logo');
    logos.forEach(function(logo) {
      logo.style.cursor = 'pointer';
      logo.addEventListener('click', function() {
        window.open('https://krv.ai', '_blank');
      });
    });
  }
  
  addLogoLink();
  
  // Re-add handler when theme switches
  const themeToggle = document.querySelector('.theme-toggle');
  if (themeToggle) {
    themeToggle.addEventListener('click', function() {
      setTimeout(addLogoLink, 100);
    });
  }
});
