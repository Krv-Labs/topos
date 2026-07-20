class Report {
  constructor(private name: string, private threshold: number = 70) {}

  classify(score: number, flags: string[]): string {
    let grade: string;
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

function run(expr: string): unknown {
  return eval(expr);
}
