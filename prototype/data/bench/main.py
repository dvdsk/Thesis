from typing import List

import numpy as np
from matplotlib import pyplot as plt

data = {
    3: [[
        14.663355, 22.119445, 42.993018, 315.477246, 346.968455, 460.445117,
        635.627372, 1.848069813, 2.550123655, 3.951157681, 8.408739796,
        25.197661361]],

    4: [[169.127803, 223.840732, 285.045141, 343.243796, 481.393309,
        588.959344, 714.290625, 964.636316, 1.346443679, 3.07621848,
        6.463473386, 8.560151802],

        [14.96779, 24.389257, 37.282205, 251.612513, 345.974787,
        413.712622, 578.770169, 830.265687, 2.194282485, 2.980160321,
        6.897937665, 13.911419252]],

    5: [[14.510684, 24.065706, 27.864601, 245.793932, 317.651037, 373.298807,
        492.465779, 777.374525, 960.578001, 2.707936301, 3.726989861,
        11.885154531]],

    6: [[
        171.079041, 228.363158, 303.942468, 449.880076, 586.687431, 762.186919,
        968.640814, 1.460654198, 2.884889332, 5.338341249, 13.100811245,
        25.118385013]],
}


def convert_ms(data: List[float]):
    prev = 0
    newdata = [i for i in data]
    for i in range(len(newdata)):
        if prev > newdata[i]:
            break
        prev = newdata[i]
        newdata[i] /= 1000
    return newdata


def x_for(y) -> List[float]:
    return [2**i for i in range(1, len(y)+1)]


def plot(ax, data):

    for cluster_size, data in data.items():
        for raw_data in data:
            measurements = convert_ms(raw_data)

            y = np.array(measurements)
            x = x_for(y)
            n_requests = np.array(x)*100
            y = y / n_requests * 1000

            ax.scatter(x, y, label=f"#nodes: {cluster_size}")


def shared_plot_markup(ax):
    label_pos = x_for(range(0, 12))
    label_text = [str(x) for x in label_pos]

    ax.set_xscale("log")
    ax.set_xticks(label_pos)
    ax.set_xticklabels(label_text)
    ax.tick_params(axis="x", which="minor", bottom=False)
    plt.xticks(rotation=45)
    plt.xlabel("# parallel connections")
    plt.ylabel("request duration (ms)")
    plt.legend()
    plt.tight_layout()


fig, ax = plt.subplots(figsize=(9, 4), sharex=True)
plot(ax, data)
shared_plot_markup(ax)
ax.set_yscale("log")
ax.set_yticks([0.025, 0.1, 0.5, 1])
ax.set_yticklabels(["0.05", "0.1", "0.5", "1"])
plt.savefig("../ls_log.png", dpi=300)
plt.close(fig)


fig, ax = plt.subplots(figsize=(9, 4), sharex=True)
plot(ax, data)
shared_plot_markup(ax)
plt.xlim(60, 4420)
plt.ylim(0, 0.129)
plt.savefig("../ls_lin.png", dpi=300)
plt.show()
