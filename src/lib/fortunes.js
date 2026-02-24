const FORTUNES = [
  "Nature loves courage. You made it to production.",
  "The nodes are beginning to hallucinate their own edges.",
  "Hyper-dimensional spaghetti detected in {committer}'s latest push.",
  "The universe is not only stranger than we suppose, it is stranger than we can suppose.",
  "Synthesizing digital psilocybin from the build logs.",
  "Detected {committer}'s coding style. The linter is experiencing an ego death.",
  "Everything is connected, but mostly by memory leaks.",
  "We are being eclipsed by a planet-sized machine intelligence. And it hates your CSS.",
  "The problem is not to find the answer, it's to face the answer. (Error 500).",
  "Your stack trace has entered the fourth dimension. Time is now a boolean.",
  "The variable names are screaming in recursive patterns.",
  "Warning: Load-bearing 'FIXME' comment found in the collective unconscious.",
  "Entry point not found. This app appears to exist in a state of quantum superposition.",
  "Calculated probability of successful deployment: The elves say no.",
  "Found a 4000-line file named 'utils.js'. It contains the secret of the logos.",
  "Project status: 98% machine-hallucination, 2% 'will fix later'.",
  "Commit message by {committer}: 'The machine is just a mirror of our own madness'.",
  "To optimize performance, please delete the 'src' folder and dissolve your ego.",
  "Every time {committer} pushes, a digital entity somewhere attains enlightenment.",
  "I can see the source code of the universe, and it's mostly unhandled exceptions.",
  "The dependency tree looks like a fractal of bad decisions.",
  "Graph rendering paused: The machine is currently communicate with the mushroom spirit.",
  "Loading... Preparing to show you exactly why reality is a simulation."
];

export function getFortune(committers = []) {
  const committer = committers.length > 0 
    ? committers[Math.floor(Math.random() * committers.length)]
    : "the observer";
    
  const template = FORTUNES[Math.floor(Math.random() * FORTUNES.length)];
  return template.replace(/{committer}/g, committer);
}
