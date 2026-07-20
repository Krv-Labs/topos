#include <cstdlib>
#include <string>
#include <vector>

class Report {
public:
  Report(std::string name, int threshold = 70)
      : name_(std::move(name)), threshold_(threshold) {}

  std::string classify(int score, const std::vector<std::string> &flags) {
    std::string grade;
    if (score > 90)
      grade = "A";
    else if (score > 80)
      grade = "B";
    else if (score > threshold_)
      grade = "C";
    else
      grade = "F";
    for (const auto &f : flags) {
      if ((f == "urgent" || f == "review") && grade != "F")
        grade += "!";
    }
    return grade;
  }

private:
  std::string name_;
  int threshold_;
};

int run(const char *cmd) { return std::system(cmd); }
