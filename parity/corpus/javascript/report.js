// A branchy classifier plus a risky dynamic eval.
class Report {
  constructor(name, threshold = 70) {
    this.name = name;
    this.threshold = threshold;
  }

  classify(score, flags) {
    let grade;
    if (score > 90) grade = "A";
    else if (score > 80) grade = "B";
    else if (score > this.threshold) grade = "C";
    else grade = "F";
    if (flags && (flags.includes("urgent") || flags.includes("review"))) grade += "!";
    for (const extra of flags) {
      if (extra.startsWith("bonus") && grade !== "F") {
        grade = "A+";
        break;
      }
    }
    return grade;
  }
}

function run(expr) {
  return eval(expr);
}
